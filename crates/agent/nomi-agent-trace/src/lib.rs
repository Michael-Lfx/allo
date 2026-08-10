//! Agent turn observability: structured traces with redacted previews and a
//! file-backed store under `{data_dir}/diagnostics/agent-traces/`.
//!
//! This crate is intentionally agent-layer only — it does **not** depend on
//! `nomi-agent` or any `nomifun-*` backend crate.

mod builder;
mod redact;
mod reported;
mod session;
mod store;
mod types;

pub use builder::{TokenCounts, TurnTraceBuilder, TurnTraceMeta};
pub use redact::{
    redact_json_value, redact_preview, truncate_chars, MAX_PREVIEW_CHARS,
};
pub use reported::{
    collect_paths_from_tool_args, is_file_mutation_tool_name, normalize_reported_path,
    parse_paths_from_tool_output, reported_artifacts_from_tool_call,
    reported_artifacts_from_tool_span,
};
pub use session::{classify_session_kind, is_session_dialogue};
pub use store::{FileTraceStore, TraceStoreError};
pub use types::{
    SpanKind, SpanStatus, TraceArtifactIndexEntry, TraceArtifactMeta, TraceIndexEntry, TraceSpan,
    TurnSummary, TurnTrace, SCHEMA_VERSION,
};
