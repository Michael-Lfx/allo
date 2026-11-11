//! AI Mate Voice Companion Library

pub mod config;
pub mod net_preboot;
pub mod oobe;
pub mod provision;
pub mod self_check_audio;
pub mod server;
pub mod voice;
#[cfg(feature = "voice-skeleton")]
pub use nomifun_voice_terminal as replay_terminal;
mod voice_events;
mod voice_feedback;
pub mod watchdog;
