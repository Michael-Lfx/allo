//! Product telemetry with property allowlist, opt-out, and a local outbox.
//!
//! Ported from allo's UI analytics contract (no PostHog/Sentry React SDKs).
//! Events are written to `{data_dir}/telemetry/outbox.jsonl` and mirrored to
//! `tracing`. Optional `PostHog` capture uses `FLOWY_POSTHOG_KEY` /
//! `FLOWY_POSTHOG_HOST` when configured and not opted out.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub type ProductEventProperty = Value;

const OPT_OUT_FILE: &str = "telemetry_opt_out";
const INSTALL_ID_FILE: &str = "install_id";
const OUTBOX_FILE: &str = "outbox.jsonl";
const OUTBOX_CAP: usize = 500;
const STRING_MAX: usize = 256;

fn allowed_properties() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        HashSet::from([
            "accept_ms",
            "already_ready",
            "average_bps",
            "blocker",
            "bytes_total",
            "bytes_transferred",
            "cdn_host",
            "cold_start",
            "duration_ms",
            "duration_secs",
            "error_code",
            "error_message",
            "failure_class",
            "failure_code",
            "feature",
            "from_version",
            "kind",
            "locale",
            "outcome",
            "peak_bps",
            "phase",
            "source",
            "status",
            "to_version",
            "total_ms",
            "tz_offset_min",
            "viewport",
        ])
    })
}

#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    pub data_dir: PathBuf,
    pub opt_out: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct OutboxEvent {
    event_id: String,
    name: String,
    distinct_id: String,
    ts_unix_ms: u64,
    properties: Map<String, Value>,
}

/// Process-wide telemetry handle. Safe to clone via Arc outside this crate.
pub struct Telemetry {
    data_dir: PathBuf,
    opt_out: Mutex<bool>,
    install_id: String,
}

impl std::fmt::Debug for Telemetry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Telemetry")
            .field("data_dir", &self.data_dir)
            .field("install_id", &self.install_id)
            .finish_non_exhaustive()
    }
}

impl Telemetry {
    pub fn open(config: TelemetryConfig) -> Self {
        let root = config.data_dir.join("telemetry");
        let _ = fs::create_dir_all(&root);
        let install_id = load_or_create_install_id(&root);
        let opt_out = if config.opt_out {
            true
        } else {
            root.join(OPT_OUT_FILE).is_file()
        };
        if config.opt_out {
            let _ = fs::write(root.join(OPT_OUT_FILE), b"1");
        }
        Self {
            data_dir: root,
            opt_out: Mutex::new(opt_out),
            install_id,
        }
    }

    #[must_use]
    pub fn install_id(&self) -> &str {
        &self.install_id
    }

    #[must_use]
    pub fn is_opted_out(&self) -> bool {
        *self
            .opt_out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_opt_out(&self, opt_out: bool) {
        *self
            .opt_out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = opt_out;
        let marker = self.data_dir.join(OPT_OUT_FILE);
        if opt_out {
            let _ = fs::write(marker, b"1");
        } else {
            let _ = fs::remove_file(marker);
        }
    }

    pub fn capture(&self, name: &str, properties: Map<String, Value>) {
        if self.is_opted_out() {
            return;
        }
        let properties = sanitize_properties(properties);
        let event = OutboxEvent {
            event_id: new_id(),
            name: name.to_owned(),
            distinct_id: self.install_id.clone(),
            ts_unix_ms: now_ms(),
            properties,
        };
        tracing::info!(
            target: "flowy.telemetry",
            event = %event.name,
            distinct_id = %event.distinct_id,
            props = ?event.properties,
            "product event"
        );
        append_outbox(&self.data_dir, &event);
        maybe_post_posthog(&event);
    }
}

pub fn sanitize_properties(
    properties: Map<String, Value>,
) -> Map<String, Value> {
    let allowed = allowed_properties();
    let mut out = Map::new();
    for (key, value) in properties {
        if !allowed.contains(key.as_str()) {
            continue;
        }
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => {
                out.insert(key, value);
            }
            Value::String(text) => {
                let clipped: String = text.chars().take(STRING_MAX).collect();
                out.insert(key, Value::String(clipped));
            }
            _ => {}
        }
    }
    out
}

pub fn capture_exception(error: &dyn std::error::Error) {
    tracing::error!(target: "flowy.crash", error = %error, "captured exception");
    sentry::capture_error(error);
}

fn load_or_create_install_id(root: &Path) -> String {
    let path = root.join(INSTALL_ID_FILE);
    if let Ok(existing) = fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if trimmed.len() >= 8 {
            return trimmed.to_owned();
        }
    }
    let id = new_id();
    let _ = fs::write(path, &id);
    id
}

fn append_outbox(root: &Path, event: &OutboxEvent) {
    let path = root.join(OUTBOX_FILE);
    let Ok(line) = serde_json::to_string(event) else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path)
    else {
        return;
    };
    let _ = writeln!(file, "{line}");
    trim_outbox(&path);
}

fn trim_outbox(path: &Path) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> =
        text.lines().filter(|line| !line.is_empty()).collect();
    if lines.len() <= OUTBOX_CAP {
        return;
    }
    let keep = &lines[lines.len() - OUTBOX_CAP..];
    let _ = flowy_core::persistence::atomic_write(
        path,
        format!("{}\n", keep.join("\n")).as_bytes(),
    );
}

fn maybe_post_posthog(event: &OutboxEvent) {
    let Ok(key) = std::env::var("FLOWY_POSTHOG_KEY") else {
        return;
    };
    let key = key.trim();
    if key.is_empty() {
        return;
    }
    let host = std::env::var("FLOWY_POSTHOG_HOST")
        .unwrap_or_else(|_| "https://us.i.posthog.com".into());
    let host = host.trim().trim_end_matches('/');
    let url = format!("{host}/capture/");
    let body = serde_json::json!({
        "api_key": key,
        "event": event.name,
        "distinct_id": event.distinct_id,
        "properties": event.properties,
        "timestamp": event.ts_unix_ms,
    });
    let Ok(client) = flowy_net::configure(
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5)),
    )
    .build() else {
        return;
    };
    let _ = client.post(url).json(&body).send();
}

fn new_id() -> String {
    let mut bytes = [0_u8; 16];
    let _ = getrandom::fill(&mut bytes);
    let mut out = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("flowy-obs-{nanos}-{id}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn sanitize_drops_unknown_and_clips_strings() {
        let mut props = Map::new();
        props.insert("phase".into(), Value::String("downloading".into()));
        props.insert("secret".into(), Value::String("nope".into()));
        props.insert("error_message".into(), Value::String("x".repeat(400)));
        let cleaned = sanitize_properties(props);
        assert_eq!(
            cleaned.get("phase").and_then(Value::as_str),
            Some("downloading")
        );
        assert!(!cleaned.contains_key("secret"));
        assert_eq!(
            cleaned
                .get("error_message")
                .and_then(Value::as_str)
                .map(str::len),
            Some(STRING_MAX)
        );
    }

    #[test]
    fn opt_out_blocks_outbox_writes() {
        let dir = scratch();
        let telemetry = Telemetry::open(TelemetryConfig {
            data_dir: dir.clone(),
            opt_out: false,
        });
        telemetry.set_opt_out(true);
        let mut props = Map::new();
        props.insert("phase".into(), Value::String("ready".into()));
        telemetry.capture("update_funnel", props);
        assert!(!dir.join("telemetry").join(OUTBOX_FILE).is_file());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn install_id_persists() {
        let dir = scratch();
        let first = Telemetry::open(TelemetryConfig {
            data_dir: dir.clone(),
            opt_out: false,
        });
        let id = first.install_id().to_owned();
        let second = Telemetry::open(TelemetryConfig {
            data_dir: dir.clone(),
            opt_out: false,
        });
        assert_eq!(second.install_id(), id);
        let _ = fs::remove_dir_all(dir);
    }
}
