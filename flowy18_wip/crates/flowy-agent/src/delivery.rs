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

/// Explicit V0 exemption code for known non-deck formats whose dedicated
/// structural verifiers have not been wired yet (`BLOCKED.md` §4).
pub const EXEMPT_NO_VERIFIER_V0: &str = "EXEMPT_NO_VERIFIER_V0";

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

/// One unresolved `deliver_intent` record found in `delivery/events.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InDoubtDelivery {
    pub delivery_id: String,
    pub artifact_path: String,
    pub artifact_hash: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryOutcome {
    pub delivery_id: String,
    pub delivered: bool,
    pub blocked: Option<BlockReason>,
    pub report_id: Option<String>,
    pub override_used: bool,
    pub exemption_code: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryResult<T> {
    pub outcome: DeliveryOutcome,
    pub value: Option<T>,
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
        code: String,
        issues: Vec<Issue>,
        at_ms: u64,
    },
    DeliverExempt {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        exemption_code: String,
        at_ms: u64,
    },
    DeliverOverride {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        code: String,
        at_ms: u64,
    },
    DeliverIntent {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        at_ms: u64,
    },
    Deliver {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        at_ms: u64,
    },
}

/// Known formats that explicitly pass under [`EXEMPT_NO_VERIFIER_V0`] when no
/// verification report exists yet. `.pptx` and unknown extensions return `None`.
#[must_use]
pub fn exemption_code_for_path(artifact: &Path) -> Option<&'static str> {
    let ext = artifact
        .extension()
        .and_then(|ext| ext.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "csv" | "pdf" | "md" | "txt" | "json" | "html" => {
            Some(EXEMPT_NO_VERIFIER_V0)
        }
        _ => None,
    }
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

    /// Persist a verification report for already-parsed `bytes`, binding the
    /// recorded SHA-256 directly to the bytes that were verified (no re-open).
    pub fn record_report_bytes(
        &self,
        artifact: &Path,
        bytes: &[u8],
        issues: Vec<Issue>,
        now_ms: u64,
    ) -> io::Result<Report> {
        let artifact_hash = hash_bytes(bytes);
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

    /// Persist a verification report and its `verify` event.
    pub fn record_report(
        &self,
        artifact: &Path,
        issues: Vec<Issue>,
        now_ms: u64,
    ) -> io::Result<Report> {
        let bytes = std::fs::read(artifact)?;
        self.record_report_bytes(artifact, &bytes, issues, now_ms)
    }

    /// Latest persisted report for `artifact`, if any.
    pub fn latest_report(&self, artifact: &Path) -> io::Result<Option<Report>> {
        let key = artifact_key(artifact);
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
            if artifact_key(Path::new(&report.artifact_path)) == key {
                latest = Some(report);
            }
        }
        Ok(latest)
    }

    fn read_events(&self) -> io::Result<Vec<AuditEvent>> {
        let bytes = match std::fs::read(self.events_path()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        let text = String::from_utf8_lossy(&bytes);
        Ok(text
            .lines()
            .filter_map(|line| serde_json::from_str::<AuditEvent>(line).ok())
            .collect())
    }

    /// Return all deliveries currently stuck in doubt (`deliver_intent`
    /// persisted with no subsequent `deliver`), ordered chronologically.
    pub fn in_doubt_deliveries(&self) -> io::Result<Vec<InDoubtDelivery>> {
        let mut pending: std::collections::BTreeMap<String, InDoubtDelivery> =
            std::collections::BTreeMap::new();
        for event in self.read_events()? {
            match event {
                AuditEvent::DeliverIntent {
                    delivery_id,
                    artifact_path,
                    artifact_hash,
                    at_ms,
                } => {
                    pending.insert(
                        delivery_id.clone(),
                        InDoubtDelivery {
                            delivery_id,
                            artifact_path,
                            artifact_hash,
                            at_ms,
                        },
                    );
                }
                AuditEvent::Deliver { delivery_id, .. } => {
                    pending.remove(&delivery_id);
                }
                _ => {}
            }
        }
        let mut list: Vec<InDoubtDelivery> = pending.into_values().collect();
        list.sort_by_key(|item| item.at_ms);
        Ok(list)
    }

    /// All content hashes delivered through an explicit user override
    /// (`deliver_override` + `deliver`), read in a single pass over the audit log.
    pub fn overridden_artifact_hashes(
        &self,
    ) -> io::Result<std::collections::BTreeSet<String>> {
        let events = self.read_events()?;
        let mut overridden_ids = std::collections::BTreeSet::new();
        for event in &events {
            if let AuditEvent::DeliverOverride { delivery_id, .. } = event {
                overridden_ids.insert(delivery_id.as_str());
            }
        }
        if overridden_ids.is_empty() {
            return Ok(std::collections::BTreeSet::new());
        }
        Ok(events
            .iter()
            .filter_map(|event| match event {
                AuditEvent::Deliver {
                    delivery_id,
                    artifact_hash,
                    ..
                } if overridden_ids.contains(delivery_id.as_str()) => {
                    Some(artifact_hash.clone())
                }
                _ => None,
            })
            .collect())
    }

    /// Whether any completed delivery for `artifact_hash` bypassed verification
    /// through an explicit user override (`deliver_override` + `deliver`).
    pub fn artifact_was_overridden(
        &self,
        artifact_hash: &str,
    ) -> io::Result<bool> {
        Ok(self.overridden_artifact_hashes()?.contains(artifact_hash))
    }

    /// Check a report without sending anything. `None` means the artifact is
    /// clear to leave; `Some(reason)` must block it.
    pub fn check(&self, artifact: &Path) -> io::Result<Option<BlockReason>> {
        let bytes = std::fs::read(artifact)?;
        let artifact_hash = hash_bytes(&bytes);
        Ok(match self.latest_report(artifact)? {
            None if exemption_code_for_path(artifact).is_some() => None,
            None => Some(BlockReason::MissingReport),
            Some(report) if !report.ok => Some(BlockReason::VerificationFailed),
            Some(report) if report.artifact_hash != artifact_hash => {
                Some(BlockReason::HashMismatch)
            }
            Some(_) => None,
        })
    }

    /// Deliver `artifact` through `send`, after checking the report, hash, or
    /// explicit exemption table, recording `deliver_intent` before `send` and
    /// `deliver` after `send` succeeds.
    pub fn deliver_with<T, F>(
        &self,
        artifact: &Path,
        allow_override: bool,
        now_ms: u64,
        send: F,
    ) -> io::Result<DeliveryResult<T>>
    where
        F: FnOnce(&[u8]) -> io::Result<T>,
    {
        let delivery_id = format!("delivery-{}-{}", now_ms, next_id());
        let bytes = std::fs::read(artifact)?;
        let artifact_hash = hash_bytes(&bytes);
        let report = self.latest_report(artifact)?;
        let exemption = if report.is_none() {
            exemption_code_for_path(artifact)
        } else {
            None
        };
        let blocked = match &report {
            None if exemption.is_some() => None,
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
        let path_display = artifact.display().to_string();
        if let Some(reason) = blocked {
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverBlocked {
                    delivery_id: delivery_id.clone(),
                    artifact_path: path_display.clone(),
                    artifact_hash: artifact_hash.clone(),
                    code: reason.code().to_owned(),
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
                return Ok(DeliveryResult {
                    outcome: DeliveryOutcome {
                        delivery_id,
                        delivered: false,
                        blocked: Some(reason),
                        report_id,
                        override_used: false,
                        exemption_code: None,
                    },
                    value: None,
                });
            }
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverOverride {
                    delivery_id: delivery_id.clone(),
                    artifact_path: path_display.clone(),
                    artifact_hash: artifact_hash.clone(),
                    code: reason.code().to_owned(),
                    at_ms: now_ms,
                },
            )?;
        } else if let Some(exemption_code) = exemption {
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverExempt {
                    delivery_id: delivery_id.clone(),
                    artifact_path: path_display.clone(),
                    artifact_hash: artifact_hash.clone(),
                    exemption_code: exemption_code.to_owned(),
                    at_ms: now_ms,
                },
            )?;
        }
        Self::append_json(
            &self.events_path(),
            &AuditEvent::DeliverIntent {
                delivery_id: delivery_id.clone(),
                artifact_path: path_display.clone(),
                artifact_hash: artifact_hash.clone(),
                at_ms: now_ms,
            },
        )?;
        let value = send(&bytes)?;
        Self::append_json(
            &self.events_path(),
            &AuditEvent::Deliver {
                delivery_id: delivery_id.clone(),
                artifact_path: path_display,
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
            exemption_code = exemption.unwrap_or(""),
            "delivery sent"
        );
        Ok(DeliveryResult {
            outcome: DeliveryOutcome {
                delivery_id,
                delivered: true,
                blocked,
                report_id,
                override_used: blocked.is_some(),
                exemption_code: exemption,
            },
            value: Some(value),
        })
    }

    /// Explicit user override bound to one verified report and its exact file
    /// hash (§8.3). Refuses missing reports, stale `expected_report_id`, and
    /// any file whose bytes changed since the report was recorded.
    pub fn deliver_override_for_report<T, F>(
        &self,
        artifact: &Path,
        expected_report_id: &str,
        now_ms: u64,
        send: F,
    ) -> io::Result<DeliveryResult<T>>
    where
        F: FnOnce(&[u8]) -> io::Result<T>,
    {
        let delivery_id = format!("delivery-{}-{}", now_ms, next_id());
        let bytes = std::fs::read(artifact)?;
        let artifact_hash = hash_bytes(&bytes);
        let path_display = artifact.display().to_string();
        let report = self.latest_report(artifact)?;
        let hard_block = match &report {
            None => Some(BlockReason::MissingReport),
            Some(report)
                if (!expected_report_id.is_empty()
                    && report.id != expected_report_id)
                    || report.artifact_hash != artifact_hash =>
            {
                Some(BlockReason::HashMismatch)
            }
            Some(_) => None,
        };
        let issues = report
            .as_ref()
            .map_or_else(Vec::new, |report| report.issues.clone());
        let report_id = report.as_ref().map(|report| report.id.clone());
        if let Some(reason) = hard_block {
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverBlocked {
                    delivery_id: delivery_id.clone(),
                    artifact_path: path_display,
                    artifact_hash: artifact_hash.clone(),
                    code: reason.code().to_owned(),
                    issues,
                    at_ms: now_ms,
                },
            )?;
            tracing::info!(
                target: "flowy.delivery",
                delivery_id = %delivery_id,
                artifact_hash = %artifact_hash,
                outcome = "blocked",
                code = reason.code(),
                "override delivery rejected: report missing or artifact changed"
            );
            return Ok(DeliveryResult {
                outcome: DeliveryOutcome {
                    delivery_id,
                    delivered: false,
                    blocked: Some(reason),
                    report_id,
                    override_used: false,
                    exemption_code: None,
                },
                value: None,
            });
        }
        let report = report.expect("report checked above");
        let override_used = !report.ok;
        let blocked = override_used.then_some(BlockReason::VerificationFailed);
        if override_used {
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverBlocked {
                    delivery_id: delivery_id.clone(),
                    artifact_path: path_display.clone(),
                    artifact_hash: artifact_hash.clone(),
                    code: BlockReason::VerificationFailed.code().to_owned(),
                    issues,
                    at_ms: now_ms,
                },
            )?;
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverOverride {
                    delivery_id: delivery_id.clone(),
                    artifact_path: path_display.clone(),
                    artifact_hash: artifact_hash.clone(),
                    code: BlockReason::VerificationFailed.code().to_owned(),
                    at_ms: now_ms,
                },
            )?;
        }
        Self::append_json(
            &self.events_path(),
            &AuditEvent::DeliverIntent {
                delivery_id: delivery_id.clone(),
                artifact_path: path_display.clone(),
                artifact_hash: artifact_hash.clone(),
                at_ms: now_ms,
            },
        )?;
        let value = send(&bytes)?;
        Self::append_json(
            &self.events_path(),
            &AuditEvent::Deliver {
                delivery_id: delivery_id.clone(),
                artifact_path: path_display,
                artifact_hash: artifact_hash.clone(),
                at_ms: now_ms,
            },
        )?;
        tracing::info!(
            target: "flowy.delivery",
            delivery_id = %delivery_id,
            artifact_hash = %artifact_hash,
            outcome = "delivered",
            override_used,
            exemption_code = "",
            "delivery sent"
        );
        Ok(DeliveryResult {
            outcome: DeliveryOutcome {
                delivery_id,
                delivered: true,
                blocked,
                report_id,
                override_used,
                exemption_code: None,
            },
            value: Some(value),
        })
    }
}

#[allow(clippy::redundant_pub_crate)]
pub(crate) fn artifact_key(path: &Path) -> String {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let resolved = std::fs::canonicalize(&normalized).unwrap_or_else(|_| {
        if let (Some(parent), Some(name)) =
            (normalized.parent(), normalized.file_name())
            && let Ok(canon_parent) = std::fs::canonicalize(parent)
        {
            canon_parent.join(name)
        } else {
            normalized
        }
    });
    let slash = resolved.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    {
        slash.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        slash
    }
}

#[allow(clippy::redundant_pub_crate)]
pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
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

    fn deliver_outcome<F>(
        store: &DeliveryStore,
        path: &Path,
        allow_override: bool,
        now_ms: u64,
        send: F,
    ) -> DeliveryOutcome
    where
        F: FnOnce(&[u8]) -> io::Result<()>,
    {
        store
            .deliver_with(path, allow_override, now_ms, send)
            .unwrap()
            .outcome
    }

    #[test]
    fn a_missing_report_blocks_without_calling_the_sender() {
        let root = scratch("missing");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK deck");
        let (calls, send) = counting_sender();
        let outcome = deliver_outcome(&store, &path, false, 1_000, send);
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
        let outcome = deliver_outcome(&store, &path, false, 2_000, send);
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
        let outcome = deliver_outcome(&store, &path, false, 2_000, send);
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
        let outcome =
            deliver_outcome(&store, &path, false, 2_000, move |bytes| {
                sink.lock().unwrap().extend_from_slice(bytes);
                Ok(())
            });
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
        let outcome = deliver_outcome(&store, &path, true, 1_000, send);
        assert!(outcome.delivered);
        assert!(outcome.override_used);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let expected_hash = hash_bytes(b"PK deck");
        assert!(
            store
                .overridden_artifact_hashes()
                .unwrap()
                .contains(&expected_hash)
        );
        assert!(store.artifact_was_overridden(&expected_hash).unwrap());
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
        let _ = deliver_outcome(&store, &path, false, 1_000, send);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let events = std::fs::read_to_string(store.events_path()).unwrap();
        assert!(events.contains("deliver_blocked"));
        assert!(!events.contains("\"deliver\""));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normalized_paths_match_the_same_verification_report() {
        let root = scratch("norm");
        let store = DeliveryStore::new(root.join("delivery"));
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("deck.pptx");
        std::fs::write(&file, b"PK normalized").unwrap();
        let slash_form =
            PathBuf::from(file.display().to_string().replace('\\', "/"));
        store.record_report(&slash_form, Vec::new(), 1_000).unwrap();
        let dot_form = sub.join(".").join("deck.pptx");
        assert_eq!(store.check(&dot_form).unwrap(), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exempt_extensions_pass_with_explicit_audit_while_unknown_extensions_block()
     {
        let root = scratch("exempt");
        let store = DeliveryStore::new(root.join("delivery"));
        let csv = root.join("sales.csv");
        std::fs::write(&csv, b"a,b\n1,2\n").unwrap();
        let (csv_calls, csv_send) = counting_sender();
        let csv_outcome = deliver_outcome(&store, &csv, false, 1_000, csv_send);
        assert!(csv_outcome.delivered);
        assert_eq!(csv_outcome.exemption_code, Some(EXEMPT_NO_VERIFIER_V0));
        assert_eq!(csv_calls.load(Ordering::SeqCst), 1);

        let unknown = root.join("bundle.bin");
        std::fs::write(&unknown, b"\x00\x01\x02").unwrap();
        let (bin_calls, bin_send) = counting_sender();
        let bin_outcome =
            deliver_outcome(&store, &unknown, false, 2_000, bin_send);
        assert!(!bin_outcome.delivered);
        assert_eq!(bin_outcome.blocked, Some(BlockReason::MissingReport));
        assert_eq!(bin_calls.load(Ordering::SeqCst), 0);

        let events = std::fs::read_to_string(store.events_path()).unwrap();
        assert!(events.contains("deliver_exempt"));
        assert!(events.contains(EXEMPT_NO_VERIFIER_V0));
        assert!(events.contains("deliver_blocked"));
        assert_eq!(store.in_doubt_deliveries().unwrap().as_slice(), []);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_sender_failure_leaves_deliver_intent_in_doubt_without_deliver() {
        let root = scratch("in-doubt");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK verified");
        store.record_report(&path, Vec::new(), 1_000).unwrap();
        let result = store.deliver_with(&path, false, 2_000, |_bytes| {
            Err::<(), _>(io::Error::other("simulated crash mid-send"))
        });
        assert!(result.is_err());
        let events = store.read_events().unwrap();
        let intent_id = events
            .iter()
            .find_map(|event| {
                if let AuditEvent::DeliverIntent { delivery_id, .. } = event {
                    Some(delivery_id.clone())
                } else {
                    None
                }
            })
            .expect("deliver_intent must be persisted before send");
        let unresolved = store.in_doubt_deliveries().unwrap();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].delivery_id, intent_id);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn report_override_blocks_missing_stale_and_tampered_artifacts() {
        let root = scratch("override-bound");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK original");
        let (calls, send) = counting_sender();

        // Missing report must be hard-blocked even on override.
        let missing = store
            .deliver_override_for_report(&path, "report-none", 1_000, send)
            .unwrap();
        assert!(!missing.outcome.delivered);
        assert_eq!(missing.outcome.blocked, Some(BlockReason::MissingReport));
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // Record a failed report for the original bytes.
        let failed = store
            .record_report_bytes(
                &path,
                b"PK original",
                vec![Issue {
                    code: "STRUCTURE".to_owned(),
                    field: "brief_min_slides".to_owned(),
                }],
                2_000,
            )
            .unwrap();

        // Stale report_id must be rejected.
        let (stale_calls, stale_send) = counting_sender();
        let stale = store
            .deliver_override_for_report(
                &path,
                "report-stale",
                2_500,
                stale_send,
            )
            .unwrap();
        assert!(!stale.outcome.delivered);
        assert_eq!(stale.outcome.blocked, Some(BlockReason::HashMismatch));
        assert_eq!(stale_calls.load(Ordering::SeqCst), 0);

        // Tampering with file bytes after the failed card opened must be rejected.
        std::fs::write(&path, b"PK swapped-bytes").unwrap();
        let (swap_calls, swap_send) = counting_sender();
        let swapped = store
            .deliver_override_for_report(&path, &failed.id, 3_000, swap_send)
            .unwrap();
        assert!(!swapped.outcome.delivered);
        assert_eq!(swapped.outcome.blocked, Some(BlockReason::HashMismatch));
        assert_eq!(swap_calls.load(Ordering::SeqCst), 0);

        // Restoring the exact bytes bound to `failed.id` allows explicit override.
        std::fs::write(&path, b"PK original").unwrap();
        let (ok_calls, ok_send) = counting_sender();
        let allowed = store
            .deliver_override_for_report(&path, &failed.id, 4_000, ok_send)
            .unwrap();
        assert!(allowed.outcome.delivered);
        assert!(allowed.outcome.override_used);
        assert_eq!(ok_calls.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
