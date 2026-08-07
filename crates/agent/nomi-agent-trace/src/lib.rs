//! Agent turn observability: structured traces with redacted previews and a
//! file-backed store under `{data_dir}/diagnostics/agent-traces/`.
//!
//! This crate is intentionally agent-layer only — it does **not** depend on
//! `nomi-agent` or any `nomifun-*` backend crate.

mod builder;
mod redact;
mod session;
mod store;
mod types;

pub use builder::{TokenCounts, TurnTraceBuilder, TurnTraceMeta};
pub use redact::{
    redact_json_value, redact_preview, truncate_chars, MAX_PREVIEW_CHARS,
};
pub use session::{classify_session_kind, is_session_dialogue};
pub use store::{FileTraceStore, TraceStoreError};
pub use types::{
    SpanKind, SpanStatus, TraceIndexEntry, TraceSpan, TurnSummary, TurnTrace, SCHEMA_VERSION,
};
