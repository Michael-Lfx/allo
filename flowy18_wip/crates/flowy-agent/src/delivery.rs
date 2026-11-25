//! Delivery gate: the only way an artifact leaves the host.
//!
//! The gate re-reads its own verification report, recomputes the artifact
//! hash from the bytes it is about to send, and writes an append-only audit
//! trail (`verify` / `deliver_blocked` / `deliver_override` / `deliver`).
//! A blocked delivery never calls the sender.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One verification finding, as reported by `verify_deck`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub code: String,
    pub field: String,
}

/// A persisted verification report. Keyed by artifact path; the hash inside
/// is what the gate re-checks before delivering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub artifact_path: String,
    pub artifact_hash: String,
    pub ok: bool,
    pub issues: Vec<Issue>,
    pub at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockReason {
    MissingReport,
    VerificationFailed,
    HashMismatch,
}

impl BlockReason {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingReport => "DELIVERY_NOT_VERIFIED",
            Self::VerificationFailed => "DELIVERY_VERIFICATION_FAILED",
            Self::HashMismatch => "DELIVERY_ARTIFACT_CHANGED",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryOutcome {
    pub delivery_id: String,
    pub delivered: bool,
    pub blocked: Option<BlockReason>,
    pub report_id: Option<String>,
    pub override_used: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum AuditEvent {
    Verify {
        delivery_id: String,
        report_id: String,
        artifact_path: String,
        artifact_hash: String,
        ok: bool,
        issues: Vec<Issue>,
        at_ms: u64,
    },
    DeliverBlocked {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        code: &'static str,
        issues: Vec<Issue>,
        at_ms: u64,
    },
    DeliverOverride {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        code: &'static str,
        at_ms: u64,
    },
    Deliver {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        at_ms: u64,
    },
}

/// Append-only store for reports and delivery events, rooted at the store
/// directory (typically `<store>/delivery`).
#[derive(Clone, Debug)]
pub struct DeliveryStore {
    root: PathBuf,
}

impl DeliveryStore {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn reports_path(&self) -> PathBuf {
        self.root.join("reports.jsonl")
    }

    fn events_path(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }

    fn append_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let line = serde_json::to_string(value)
            .map_err(|error| io::Error::other(error.to_string()))?;
        writeln!(file, "{line}")
    }

    /// Persist a verification report and its `verify` event.
    pub fn record_report(
        &self,
        artifact: &Path,
        issues: Vec<Issue>,
        now_ms: u64,
    ) -> io::Result<Report> {
        let bytes = std::fs::read(artifact)?;
        let artifact_hash = hash_bytes(&bytes);
        let report_id = format!("report-{}-{}", now_ms, next_id());
        let report = Report {
            id: report_id.clone(),
            artifact_path: artifact.display().to_string(),
            artifact_hash: artifact_hash.clone(),
            ok: issues.is_empty(),
            issues,
            at_ms: now_ms,
        };
        Self::append_json(&self.reports_path(), &report)?;
        Self::append_json(
            &self.events_path(),
            &AuditEvent::Verify {
                delivery_id: String::new(),
                report_id,
                artifact_path: report.artifact_path.clone(),
                artifact_hash,
                ok: report.ok,
                issues: report.issues.clone(),
                at_ms: now_ms,
            },
        )?;
        Ok(report)
    }

    fn latest_report(&self, artifact: &Path) -> io::Result<Option<Report>> {
        let key = artifact.display().to_string();
        let bytes = match std::fs::read(self.reports_path()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut latest = None;
        for line in text.lines() {
            let Ok(report) = serde_json::from_str::<Report>(line) else {
                continue;
            };
            if report.artifact_path == key {
                latest = Some(report);
            }
        }
        Ok(latest)
    }

    /// Check a report without sending anything. `None` means the artifact is
    /// clear to leave; `Some(reason)` must block it.
    pub fn check(&self, artifact: &Path) -> io::Result<Option<BlockReason>> {
        let bytes = std::fs::read(artifact)?;
        let artifact_hash = hash_bytes(&bytes);
        Ok(match self.latest_report(artifact)? {
            None => Some(BlockReason::MissingReport),
            Some(report) if !report.ok => Some(BlockReason::VerificationFailed),
            Some(report) if report.artifact_hash != artifact_hash => {
                Some(BlockReason::HashMismatch)
            }
            Some(_) => None,
        })
    }

    /// Deliver `artifact` through `send`, after checking the report and hash.
    pub fn deliver<F>(
        &self,
        artifact: &Path,
        allow_override: bool,
        now_ms: u64,
        send: F,
    ) -> io::Result<DeliveryOutcome>
    where
        F: FnOnce(&[u8]) -> io::Result<()>,
    {
        let delivery_id = format!("delivery-{}-{}", now_ms, next_id());
        let bytes = std::fs::read(artifact)?;
        let artifact_hash = hash_bytes(&bytes);
        let report = self.latest_report(artifact)?;
        let blocked = match &report {
            None => Some(BlockReason::MissingReport),
            Some(report) if !report.ok => Some(BlockReason::VerificationFailed),
            Some(report) if report.artifact_hash != artifact_hash => {
                Some(BlockReason::HashMismatch)
            }
            Some(_) => None,
        };
        let issues = report
            .as_ref()
            .map_or_else(Vec::new, |report| report.issues.clone());
        let report_id = report.as_ref().map(|report| report.id.clone());
        if let Some(reason) = blocked {
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverBlocked {
                    delivery_id: delivery_id.clone(),
                    artifact_path: artifact.display().to_string(),
                    artifact_hash: artifact_hash.clone(),
                    code: reason.code(),
                    issues,
                    at_ms: now_ms,
                },
            )?;
            if !allow_override {
                tracing::info!(
                    target: "flowy.delivery",
                    delivery_id = %delivery_id,
                    artifact_hash = %artifact_hash,
                    outcome = "blocked",
                    code = reason.code(),
                    "delivery blocked"
                );
                return Ok(DeliveryOutcome {
                    delivery_id,
                    delivered: false,
                    blocked: Some(reason),
                    report_id,
                    override_used: false,
                });
            }
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverOverride {
                    delivery_id: delivery_id.clone(),
                    artifact_path: artifact.display().to_string(),
                    artifact_hash: artifact_hash.clone(),
                    code: reason.code(),
                    at_ms: now_ms,
                },
            )?;
        }
        send(&bytes)?;
        Self::append_json(
            &self.events_path(),
            &AuditEvent::Deliver {
                delivery_id: delivery_id.clone(),
                artifact_path: artifact.display().to_string(),
                artifact_hash: artifact_hash.clone(),
                at_ms: now_ms,
            },
        )?;
        tracing::info!(
            target: "flowy.delivery",
            delivery_id = %delivery_id,
            artifact_hash = %artifact_hash,
            outcome = "delivered",
            override_used = blocked.is_some(),
            "delivery sent"
        );
        Ok(DeliveryOutcome {
            delivery_id,
            delivered: true,
            blocked,
            report_id,
            override_used: blocked.is_some(),
        })
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("flowy-delivery-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn artifact(root: &Path, body: &[u8]) -> PathBuf {
        let path = root.join("deck.pptx");
        std::fs::write(&path, body).unwrap();
        path
    }

    fn counting_sender() -> (Arc<AtomicUsize>, impl Fn(&[u8]) -> io::Result<()>)
    {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let send = move |_bytes: &[u8]| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        (calls, send)
    }

    #[test]
    fn a_missing_report_blocks_without_calling_the_sender() {
        let root = scratch("missing");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK deck");
        let (calls, send) = counting_sender();
        let outcome = store.deliver(&path, false, 1_000, send).unwrap();
        assert!(!outcome.delivered);
        assert_eq!(outcome.blocked, Some(BlockReason::MissingReport));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_report_blocks_and_keeps_the_issues() {
        let root = scratch("failed");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK deck");
        let issues = vec![Issue {
            code: "VALUE_MISMATCH".to_owned(),
            field: "S0-title".to_owned(),
        }];
        store.record_report(&path, issues, 1_000).unwrap();
        let (calls, send) = counting_sender();
        let outcome = store.deliver(&path, false, 2_000, send).unwrap();
        assert_eq!(outcome.blocked, Some(BlockReason::VerificationFailed));
        assert!(!outcome.delivered);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            store.latest_report(&path).unwrap().unwrap().issues.len(),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changed_artifact_no_longer_matches_its_report() {
        let root = scratch("changed");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK original");
        store.record_report(&path, Vec::new(), 1_000).unwrap();
        std::fs::write(&path, b"PK tampered").unwrap();
        let (calls, send) = counting_sender();
        let outcome = store.deliver(&path, false, 2_000, send).unwrap();
        assert_eq!(outcome.blocked, Some(BlockReason::HashMismatch));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_verified_artifact_is_delivered_with_the_reported_bytes() {
        let root = scratch("pass");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK verified");
        store.record_report(&path, Vec::new(), 1_000).unwrap();
        let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = sent.clone();
        let outcome = store
            .deliver(&path, false, 2_000, move |bytes| {
                sink.lock().unwrap().extend_from_slice(bytes);
                Ok(())
            })
            .unwrap();
        assert!(outcome.delivered);
        assert!(outcome.blocked.is_none());
        assert_eq!(sent.lock().unwrap().as_slice(), b"PK verified");
        assert!(outcome.report_id.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn override_is_explicit_and_leaves_both_events() {
        let root = scratch("override");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK deck");
        let (calls, send) = counting_sender();
        let outcome = store.deliver(&path, true, 1_000, send).unwrap();
        assert!(outcome.delivered);
        assert!(outcome.override_used);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let events = std::fs::read_to_string(store.events_path()).unwrap();
        assert!(events.contains("deliver_blocked"));
        assert!(events.contains("deliver_override"));
        assert!(events.contains("\"deliver\""));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_blocked_delivery_writes_no_deliver_event() {
        let root = scratch("audit");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK deck");
        let (calls, send) = counting_sender();
        let _ = store.deliver(&path, false, 1_000, send).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let events = std::fs::read_to_string(store.events_path()).unwrap();
        assert!(events.contains("deliver_blocked"));
        assert!(!events.contains("\"deliver\""));
        let _ = std::fs::remove_dir_all(&root);
    }
}
