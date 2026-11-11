//! Observability: file logging, Sentry panic reports, and privacy-scoped
//! product telemetry (allowlist + opt-out + local outbox).

#![allow(clippy::missing_const_for_fn)]

mod boot;
mod logging;
mod telemetry;

pub use boot::{BootNoteLevel, drain_boot_notes, record_boot_note};
pub use logging::{
    LogGuards, default_log_dir, init_tracing, logs_dir_for_data,
};
pub use telemetry::{
    ProductEventProperty, Telemetry, TelemetryConfig, capture_exception,
    sanitize_properties,
};
