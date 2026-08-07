//! Bounded, local-only capture of failed provider SSE streams.
//!
//! Successful streams are represented by their normalized conversation events.
//! A malformed stream has no equivalent normalized form, so retain the raw
//! bytes locally for diagnosis without copying every successful response.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use uuid::Uuid;

const MAX_CAPTURE_BYTES: usize = 256 * 1024;
const DIAGNOSTIC_DIRECTORY: &str = "diagnostics/failed-provider-sse";

#[derive(Clone, Debug)]
pub(crate) struct FailedSseCaptureContext {
    directory: PathBuf,
    provider: &'static str,
    model: String,
    turn_id: Option<String>,
}

impl FailedSseCaptureContext {
    pub(crate) fn for_openai_compatible(model: &str, turn_id: Option<String>) -> Self {
        Self {
            directory: nomi_config::data_dir().join(DIAGNOSTIC_DIRECTORY),
            provider: "openai-compatible",
            model: model.to_owned(),
            turn_id,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_directory(
        directory: PathBuf,
        provider: &'static str,
        model: impl Into<String>,
        turn_id: Option<String>,
    ) -> Self {
        Self {
            directory,
            provider,
            model: model.into(),
            turn_id,
        }
    }
}

pub(crate) struct FailedSseCapture {
    context: FailedSseCaptureContext,
    bytes: Vec<u8>,
    truncated: bool,
}

impl FailedSseCapture {
    pub(crate) fn new(context: FailedSseCaptureContext) -> Self {
        Self {
            context,
            bytes: Vec::new(),
            truncated: false,
        }
    }

    pub(crate) fn record(&mut self, chunk: &[u8]) {
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(self.bytes.len());
        let take = remaining.min(chunk.len());
        self.bytes.extend_from_slice(&chunk[..take]);
        self.truncated |= take < chunk.len();
    }

    pub(crate) fn persist(&self, reason: &str) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.context.directory).map_err(|error| {
            format!(
                "create failed-SSE diagnostic directory {}: {error}",
                self.context.directory.display()
            )
        })?;

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let stem = format!(
            "{timestamp_ms}-{}-{}",
            safe_filename_component(self.context.turn_id.as_deref().unwrap_or("unattributed")),
            Uuid::now_v7()
        );
        let sse_path = self.context.directory.join(format!("{stem}.sse"));
        let metadata_path = self.context.directory.join(format!("{stem}.json"));

        write_new_file(&sse_path, &self.bytes)?;
        let metadata = FailedSseMetadata {
            format: "nomi.failed-provider-sse.v1",
            provider: self.context.provider,
            model: &self.context.model,
            turn_id: self.context.turn_id.as_deref(),
            failure_reason: reason,
            captured_at_unix_ms: timestamp_ms,
            captured_bytes: self.bytes.len(),
            byte_limit: MAX_CAPTURE_BYTES,
            truncated: self.truncated,
            sse_file: sse_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default(),
        };
        let metadata_bytes = serde_json::to_vec_pretty(&metadata)
            .map_err(|error| format!("serialize failed-SSE metadata: {error}"))?;
        write_new_file(&metadata_path, &metadata_bytes)?;

        Ok(sse_path)
    }
}

#[derive(Serialize)]
struct FailedSseMetadata<'a> {
    format: &'static str,
    provider: &'static str,
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<&'a str>,
    failure_reason: &'a str,
    captured_at_unix_ms: u128,
    captured_bytes: usize,
    byte_limit: usize,
    truncated: bool,
    sse_file: &'a str,
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", Uuid::now_v7()));
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("write failed-SSE diagnostic {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!(
            "publish failed-SSE diagnostic {} -> {}: {error}",
            temporary.display(),
            path.display()
        )
    })
}

fn safe_filename_component(value: &str) -> String {
    let filtered: String = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(64)
        .collect();
    if filtered.is_empty() {
        "unattributed".to_owned()
    } else {
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::{FailedSseCapture, FailedSseCaptureContext, MAX_CAPTURE_BYTES};

    #[test]
    fn persists_exact_bytes_and_bounded_metadata() {
        let directory = std::env::temp_dir().join(format!("nomi-failed-sse-{}", uuid::Uuid::now_v7()));
        let context = FailedSseCaptureContext::with_directory(
            directory.clone(),
            "test-provider",
            "test-model",
            Some("turn-1".into()),
        );
        let mut capture = FailedSseCapture::new(context);
        let prefix = b"data: first\n\ndata: second\n\n";
        let overflow = vec![b'x'; MAX_CAPTURE_BYTES];
        capture.record(prefix);
        capture.record(&overflow);

        let sse_path = capture.persist("malformed tool arguments").expect("capture persists");
        assert_eq!(
            std::fs::read(&sse_path).expect("read persisted SSE"),
            [prefix.as_slice(), &overflow[..MAX_CAPTURE_BYTES - prefix.len()]].concat()
        );

        let metadata_path = sse_path.with_extension("json");
        let metadata: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&metadata_path).expect("read persisted metadata"),
        )
        .expect("metadata JSON");
        assert_eq!(metadata["provider"], "test-provider");
        assert_eq!(metadata["turn_id"], "turn-1");
        assert_eq!(metadata["failure_reason"], "malformed tool arguments");
        assert_eq!(metadata["captured_bytes"], MAX_CAPTURE_BYTES);
        assert_eq!(metadata["truncated"], true);

        std::fs::remove_dir_all(directory).expect("remove test diagnostics");
    }
}
