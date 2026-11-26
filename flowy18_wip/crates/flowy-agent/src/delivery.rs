//! Delivery gate: the only way an artifact leaves the host.
//!
//! The gate re-reads its own verification report, recomputes the artifact
//! hash from the bytes it is about to send, and writes an append-only audit
//! trail (`verify` / `deliver_blocked` / `deliver_override` / `deliver`).
//! A blocked delivery never calls the sender.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

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
    /// Work-contract revision verified by this report, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_revision: Option<u32>,
    /// Cached canonical lookup key for `artifact_path`, avoiding repeated
    /// filesystem canonicalization when scanning `reports.jsonl`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artifact_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockReason {
    MissingReport,
    VerificationFailed,
    HashMismatch,
    ContractRevisionMismatch,
}

impl BlockReason {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingReport => "DELIVERY_NOT_VERIFIED",
            Self::VerificationFailed => "DELIVERY_VERIFICATION_FAILED",
            Self::HashMismatch => "DELIVERY_ARTIFACT_CHANGED",
            Self::ContractRevisionMismatch => {
                "DELIVERY_CONTRACT_REVISION_MISMATCH"
            }
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
        #[serde(default, skip_serializing_if = "String::is_empty")]
        report_id: String,
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
    DeliverAborted {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        reason: String,
        at_ms: u64,
    },
    Deliver {
        delivery_id: String,
        artifact_path: String,
        artifact_hash: String,
        at_ms: u64,
    },
}

/// Check whether a persisted report's `contract_revision` is stale relative to
/// the turn's `expected_revision`.
///
/// Any explicitly tagged report whose revision differs from `expected` is
/// blocked. Legacy/untagged reports (`None`) remain compatible with initial
/// `rev = 1` turns, but are blocked as soon as the contract evolves (`>= 2`).
const fn contract_revision_mismatched(
    report_revision: Option<u32>,
    expected_revision: Option<u32>,
) -> bool {
    let Some(expected) = expected_revision else {
        return false;
    };
    match report_revision {
        Some(actual) => actual != expected,
        None => expected > 1,
    }
}

/// Read a regular file into memory with strict safety bounds:
/// - rejects Windows reserved device names (`CON`, `NUL`, `AUX`, `COM1`..`LPT9`),
/// - rejects non-regular files (directories, Unix FIFOs, device nodes),
/// - caps bytes read at `max_bytes` (checked both via metadata and `Read::take`).
pub(crate) fn read_regular_file_bounded(
    path: &Path,
    max_bytes: u64,
) -> io::Result<Vec<u8>> {
    if crate::handle::has_reserved_device_component(path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reserved device path refused",
        ));
    }
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || flowy_agent_core::tools::is_windows_reparse_point(&meta)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    if meta.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file exceeds maximum size limit ({max_bytes} bytes)"),
        ));
    }
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(
        usize::try_from(meta.len().min(max_bytes)).unwrap_or(0),
    );
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file exceeds maximum size limit ({max_bytes} bytes)"),
        ));
    }
    Ok(buf)
}

/// Verify that `path` points to a non-reserved regular file without loading its
/// bytes into memory.
pub(crate) fn verify_regular_file_exists(path: &Path) -> io::Result<()> {
    if crate::handle::has_reserved_device_component(path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reserved device path refused",
        ));
    }
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || flowy_agent_core::tools::is_windows_reparse_point(&meta)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    Ok(())
}

/// Stream-hash a regular file in fixed 64 KiB chunks with strict size and
/// cancellation bounds, rejecting symlinks, reparse points, and device names.
pub(crate) fn hash_regular_file_streaming(
    path: &Path,
    max_bytes: u64,
    signal: Option<&flowy_agent_core::CancelToken>,
) -> io::Result<String> {
    verify_regular_file_exists(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if meta.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file exceeds maximum size limit ({max_bytes} bytes)"),
        ));
    }
    let mut file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        if signal.is_some_and(flowy_agent_core::CancelToken::is_cancelled) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "hashing cancelled",
            ));
        }
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("file exceeds maximum size limit ({max_bytes} bytes)"),
            ));
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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

/// Pre-indexed snapshot of completed override deliveries loaded in one pass
/// over `events.jsonl`.
#[derive(Clone, Debug, Default)]
pub struct OverrideIndex {
    by_report_id: BTreeSet<String>,
    legacy_by_key_hash: BTreeSet<(String, String)>,
    legacy_hashes: BTreeSet<String>,
}

impl OverrideIndex {
    #[must_use]
    pub fn report_was_overridden(
        &self,
        report_id: &str,
        artifact_path: &str,
        artifact_hash: &str,
    ) -> bool {
        if !report_id.is_empty() && self.by_report_id.contains(report_id) {
            return true;
        }
        if !artifact_path.is_empty() && !artifact_hash.is_empty() {
            let key = artifact_key(Path::new(artifact_path));
            if self
                .legacy_by_key_hash
                .contains(&(key, artifact_hash.to_owned()))
            {
                return true;
            }
        }
        report_id.is_empty()
            && artifact_path.is_empty()
            && !artifact_hash.is_empty()
            && self.legacy_hashes.contains(artifact_hash)
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
        let lock = store_lock(path);
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        Self::append_json_locked(path, value)
    }

    fn append_json_locked<T: Serialize>(
        path: &Path,
        value: &T,
    ) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        ensure_trailing_newline(path)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let line = serde_json::to_string(value)
            .map_err(|error| io::Error::other(error.to_string()))?;
        writeln!(file, "{line}")?;
        file.flush()?;
        file.sync_all()
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
        self.record_report_bytes_with_revision(
            artifact, bytes, issues, None, now_ms,
        )
    }

    /// Persist a verification report for already-parsed `bytes`, optionally
    /// tagged with the host `WorkContract` revision it was verified against.
    pub fn record_report_bytes_with_revision(
        &self,
        artifact: &Path,
        bytes: &[u8],
        issues: Vec<Issue>,
        contract_revision: Option<u32>,
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
            contract_revision,
            artifact_key: artifact_key(artifact),
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
        let bytes =
            read_regular_file_bounded(artifact, crate::MAX_ARTIFACT_BYTES)?;
        self.record_report_bytes(artifact, &bytes, issues, now_ms)
    }

    /// Latest persisted report for `artifact`, if any.
    ///
    /// A truncated final line is tolerated, while an unparseable **interior**
    /// line invalidates any earlier report for `artifact` so corrupted failure
    /// records can never fall back to a prior passing report (fail-closed).
    pub fn latest_report(&self, artifact: &Path) -> io::Result<Option<Report>> {
        let key = artifact_key(artifact);
        let path = self.reports_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let mut latest = None;
        for (idx, line) in lines.iter().enumerate() {
            let Ok(report) = serde_json::from_str::<Report>(line) else {
                if idx + 1 < lines.len() {
                    tracing::error!(
                        target: "flowy.delivery",
                        path = %path.display(),
                        line = idx + 1,
                        "interior corruption in reports.jsonl; invalidating prior reports"
                    );
                    latest = None;
                }
                continue;
            };
            let matches_key = if report.artifact_key.is_empty() {
                artifact_key(Path::new(&report.artifact_path)) == key
            } else {
                report.artifact_key == key
            };
            if matches_key {
                latest = Some(report);
            }
        }
        Ok(latest)
    }

    fn read_events(&self) -> io::Result<Vec<AuditEvent>> {
        Self::read_events_from(&self.events_path())
    }

    fn read_events_from(path: &Path) -> io::Result<Vec<AuditEvent>> {
        let bytes = match std::fs::read(path) {
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
        let mut pending: BTreeMap<String, InDoubtDelivery> = BTreeMap::new();
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
                AuditEvent::Deliver { delivery_id, .. }
                | AuditEvent::DeliverAborted { delivery_id, .. } => {
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
    pub fn overridden_artifact_hashes(&self) -> io::Result<BTreeSet<String>> {
        let events = self.read_events()?;
        let mut overridden_ids = BTreeSet::new();
        for event in &events {
            if let AuditEvent::DeliverOverride { delivery_id, .. } = event {
                overridden_ids.insert(delivery_id.as_str());
            }
        }
        if overridden_ids.is_empty() {
            return Ok(BTreeSet::new());
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

    /// Pre-indexed snapshot of completed override deliveries loaded in a single
    /// pass over `events.jsonl`, avoiding N+1 audit log reads across cards.
    pub fn override_index(&self) -> io::Result<OverrideIndex> {
        let events = self.read_events()?;
        let delivered_ids: BTreeSet<&str> = events
            .iter()
            .filter_map(|event| match event {
                AuditEvent::Deliver { delivery_id, .. } => {
                    Some(delivery_id.as_str())
                }
                _ => None,
            })
            .collect();
        if delivered_ids.is_empty() {
            return Ok(OverrideIndex::default());
        }
        let mut by_report_id = BTreeSet::new();
        let mut legacy_by_key_hash = BTreeSet::new();
        let mut legacy_hashes = BTreeSet::new();
        for event in &events {
            if let AuditEvent::DeliverOverride {
                delivery_id,
                report_id,
                artifact_path,
                artifact_hash,
                ..
            } = event
                && delivered_ids.contains(delivery_id.as_str())
            {
                if report_id.is_empty() {
                    if !artifact_path.is_empty() && !artifact_hash.is_empty() {
                        legacy_by_key_hash.insert((
                            artifact_key(Path::new(artifact_path)),
                            artifact_hash.clone(),
                        ));
                    }
                    if !artifact_hash.is_empty() {
                        legacy_hashes.insert(artifact_hash.clone());
                    }
                } else {
                    by_report_id.insert(report_id.clone());
                }
            }
        }
        Ok(OverrideIndex {
            by_report_id,
            legacy_by_key_hash,
            legacy_hashes,
        })
    }

    /// Whether a specific verification report (or, for legacy events lacking
    /// `report_id`, the same `(artifact_key, artifact_hash)`) was delivered via
    /// an explicit user override — preventing cross-session hash collisions
    /// from prematurely marking another session's failed card as overridden.
    pub fn report_was_overridden(
        &self,
        report_id: &str,
        artifact_path: &str,
        artifact_hash: &str,
    ) -> io::Result<bool> {
        Ok(self.override_index()?.report_was_overridden(
            report_id,
            artifact_path,
            artifact_hash,
        ))
    }

    /// Whether any completed delivery for `artifact_hash` bypassed verification
    /// through an explicit user override (`deliver_override` + `deliver`).
    pub fn artifact_was_overridden(
        &self,
        artifact_hash: &str,
    ) -> io::Result<bool> {
        Ok(self.overridden_artifact_hashes()?.contains(artifact_hash))
    }

    /// Confirm a previously staged `deliver_intent` once the artifact has a
    /// visible Library marker. Idempotent per `delivery_id`.
    pub fn confirm_delivered(
        &self,
        delivery_id: &str,
        artifact_path: &str,
        artifact_hash: &str,
        now_ms: u64,
    ) -> io::Result<bool> {
        let path = self.events_path();
        let lock = store_lock(&path);
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let events = Self::read_events_from(&path)?;
        if events.iter().any(|event| {
            matches!(event, AuditEvent::Deliver { delivery_id: existing, .. } if existing == delivery_id)
        }) {
            return Ok(false);
        }
        Self::append_json_locked(
            &path,
            &AuditEvent::Deliver {
                delivery_id: delivery_id.to_owned(),
                artifact_path: artifact_path.to_owned(),
                artifact_hash: artifact_hash.to_owned(),
                at_ms: now_ms,
            },
        )?;
        Ok(true)
    }

    /// Explicitly close an unresolved local `deliver_intent` when staging or
    /// pre-commit persistence fails/cancels before any durable Library commit.
    /// Idempotent per `delivery_id` and never overrides a committed `Deliver`.
    pub fn abort_intent(
        &self,
        delivery_id: &str,
        artifact_path: &str,
        artifact_hash: &str,
        reason: &str,
        now_ms: u64,
    ) -> io::Result<bool> {
        if delivery_id.is_empty() {
            return Ok(false);
        }
        let path = self.events_path();
        let lock = store_lock(&path);
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let events = Self::read_events_from(&path)?;
        if events.iter().any(|event| {
            matches!(
                event,
                AuditEvent::Deliver { delivery_id: existing, .. }
                    | AuditEvent::DeliverAborted { delivery_id: existing, .. }
                    if existing == delivery_id
            )
        }) {
            return Ok(false);
        }
        Self::append_json_locked(
            &path,
            &AuditEvent::DeliverAborted {
                delivery_id: delivery_id.to_owned(),
                artifact_path: artifact_path.to_owned(),
                artifact_hash: artifact_hash.to_owned(),
                reason: reason.to_owned(),
                at_ms: now_ms,
            },
        )?;
        Ok(true)
    }

    /// Confirm every unresolved `deliver_intent` matching `artifact_hash` once
    /// the corresponding Library marker is durable. Idempotent across restarts.
    pub fn confirm_delivered_by_hash(
        &self,
        artifact_hash: &str,
        now_ms: u64,
    ) -> io::Result<usize> {
        if artifact_hash.is_empty() {
            return Ok(0);
        }
        let path = self.events_path();
        let lock = store_lock(&path);
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut pending: BTreeMap<String, InDoubtDelivery> = BTreeMap::new();
        for event in Self::read_events_from(&path)? {
            match event {
                AuditEvent::DeliverIntent {
                    delivery_id,
                    artifact_path,
                    artifact_hash: hash,
                    at_ms,
                } => {
                    pending.insert(
                        delivery_id.clone(),
                        InDoubtDelivery {
                            delivery_id,
                            artifact_path,
                            artifact_hash: hash,
                            at_ms,
                        },
                    );
                }
                AuditEvent::Deliver { delivery_id, .. }
                | AuditEvent::DeliverAborted { delivery_id, .. } => {
                    pending.remove(&delivery_id);
                }
                _ => {}
            }
        }
        let mut matching: Vec<InDoubtDelivery> = pending
            .into_values()
            .filter(|item| item.artifact_hash == artifact_hash)
            .collect();
        matching.sort_by_key(|item| item.at_ms);
        let mut confirmed = 0_usize;
        for item in matching {
            Self::append_json_locked(
                &path,
                &AuditEvent::Deliver {
                    delivery_id: item.delivery_id,
                    artifact_path: item.artifact_path,
                    artifact_hash: item.artifact_hash,
                    at_ms: now_ms,
                },
            )?;
            confirmed = confirmed.saturating_add(1);
        }
        Ok(confirmed)
    }

    /// Check a report without sending anything. `None` means the artifact is
    /// clear to leave; `Some(reason)` must block it.
    pub fn check(&self, artifact: &Path) -> io::Result<Option<BlockReason>> {
        self.check_for_revision(artifact, None)
    }

    /// Check a report against both artifact bytes and an optional expected
    /// `WorkContract` revision. A passing report recorded under an older
    /// contract revision is blocked with [`BlockReason::ContractRevisionMismatch`].
    pub fn check_for_revision(
        &self,
        artifact: &Path,
        expected_revision: Option<u32>,
    ) -> io::Result<Option<BlockReason>> {
        let bytes =
            read_regular_file_bounded(artifact, crate::MAX_ARTIFACT_BYTES)?;
        let artifact_hash = hash_bytes(&bytes);
        Ok(match self.latest_report(artifact)? {
            None if exemption_code_for_path(artifact).is_some() => None,
            None => Some(BlockReason::MissingReport),
            Some(report) if !report.ok => Some(BlockReason::VerificationFailed),
            Some(report) if report.artifact_hash != artifact_hash => {
                Some(BlockReason::HashMismatch)
            }
            Some(report)
                if contract_revision_mismatched(
                    report.contract_revision,
                    expected_revision,
                ) =>
            {
                Some(BlockReason::ContractRevisionMismatch)
            }
            Some(_) => None,
        })
    }

    /// Phase-1 staging gate: verify report/hash/exemption, persist
    /// `deliver_intent` before calling `stage`, and leave `deliver` unconfirmed
    /// until the Library delivery marker is durably written.
    pub fn stage_with<T, F>(
        &self,
        artifact: &Path,
        allow_override: bool,
        now_ms: u64,
        stage: F,
    ) -> io::Result<DeliveryResult<T>>
    where
        F: FnOnce(&[u8]) -> io::Result<T>,
    {
        self.stage_with_revision(artifact, allow_override, None, now_ms, stage)
    }

    /// Phase-1 staging gate enforcing `expected_contract_revision` when set.
    pub fn stage_with_revision<T, F>(
        &self,
        artifact: &Path,
        allow_override: bool,
        expected_contract_revision: Option<u32>,
        now_ms: u64,
        stage: F,
    ) -> io::Result<DeliveryResult<T>>
    where
        F: FnOnce(&[u8]) -> io::Result<T>,
    {
        Ok(self
            .stage_with_hash(
                artifact,
                allow_override,
                expected_contract_revision,
                now_ms,
                true,
                stage,
            )?
            .0)
    }

    fn stage_with_hash<T, F>(
        &self,
        artifact: &Path,
        allow_override: bool,
        expected_contract_revision: Option<u32>,
        now_ms: u64,
        abort_on_stage_error: bool,
        stage: F,
    ) -> io::Result<(DeliveryResult<T>, String)>
    where
        F: FnOnce(&[u8]) -> io::Result<T>,
    {
        let delivery_id = format!("delivery-{}-{}", now_ms, next_id());
        let bytes =
            read_regular_file_bounded(artifact, crate::MAX_ARTIFACT_BYTES)?;
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
            Some(report)
                if contract_revision_mismatched(
                    report.contract_revision,
                    expected_contract_revision,
                ) =>
            {
                Some(BlockReason::ContractRevisionMismatch)
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
                return Ok((
                    DeliveryResult {
                        outcome: DeliveryOutcome {
                            delivery_id,
                            delivered: false,
                            blocked: Some(reason),
                            report_id,
                            override_used: false,
                            exemption_code: None,
                        },
                        value: None,
                    },
                    artifact_hash,
                ));
            }
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverOverride {
                    delivery_id: delivery_id.clone(),
                    report_id: report_id.clone().unwrap_or_default(),
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
        let value = match stage(&bytes) {
            Ok(value) => value,
            Err(error) => {
                if abort_on_stage_error {
                    let _ = Self::append_json(
                        &self.events_path(),
                        &AuditEvent::DeliverAborted {
                            delivery_id,
                            artifact_path: path_display,
                            artifact_hash,
                            reason: error.to_string(),
                            at_ms: now_ms,
                        },
                    );
                }
                return Err(error);
            }
        };
        Ok((
            DeliveryResult {
                outcome: DeliveryOutcome {
                    delivery_id,
                    delivered: false,
                    blocked,
                    report_id,
                    override_used: blocked.is_some(),
                    exemption_code: exemption,
                },
                value: Some(value),
            },
            artifact_hash,
        ))
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
        let (mut result, artifact_hash) = self.stage_with_hash(
            artifact,
            allow_override,
            None,
            now_ms,
            false,
            send,
        )?;
        if result.value.is_some() {
            let path_display = artifact.display().to_string();
            self.confirm_delivered(
                &result.outcome.delivery_id,
                &path_display,
                &artifact_hash,
                now_ms,
            )?;
            result.outcome.delivered = true;
            tracing::info!(
                target: "flowy.delivery",
                delivery_id = %result.outcome.delivery_id,
                artifact_hash = %artifact_hash,
                outcome = "delivered",
                override_used = result.outcome.override_used,
                exemption_code = result.outcome.exemption_code.unwrap_or(""),
                "delivery sent"
            );
        }
        Ok(result)
    }

    /// Phase-1 staging gate for an explicit user override bound to one verified
    /// report and its exact file hash. Refuses empty or stale
    /// `expected_report_id`, missing reports, and modified files, and leaves
    /// `deliver` unconfirmed until the Library marker is durably written.
    pub fn stage_override_for_report<T, F>(
        &self,
        artifact: &Path,
        expected_report_id: &str,
        now_ms: u64,
        stage: F,
    ) -> io::Result<DeliveryResult<T>>
    where
        F: FnOnce(&[u8]) -> io::Result<T>,
    {
        self.stage_override_for_report_with_revision(
            artifact,
            expected_report_id,
            None,
            now_ms,
            stage,
        )
    }

    /// Revision-aware variant of [`Self::stage_override_for_report`] ensuring
    /// a stale `ok == true` report from an older `WorkContract` revision is
    /// audited with `DeliverBlocked` + `DeliverOverride` (`override_used = true`)
    /// rather than bypassing silently.
    pub fn stage_override_for_report_with_revision<T, F>(
        &self,
        artifact: &Path,
        expected_report_id: &str,
        expected_contract_revision: Option<u32>,
        now_ms: u64,
        stage: F,
    ) -> io::Result<DeliveryResult<T>>
    where
        F: FnOnce(&[u8]) -> io::Result<T>,
    {
        Ok(self
            .stage_override_for_report_with_hash(
                artifact,
                expected_report_id,
                expected_contract_revision,
                now_ms,
                true,
                stage,
            )?
            .0)
    }

    fn stage_override_for_report_with_hash<T, F>(
        &self,
        artifact: &Path,
        expected_report_id: &str,
        expected_contract_revision: Option<u32>,
        now_ms: u64,
        abort_on_stage_error: bool,
        stage: F,
    ) -> io::Result<(DeliveryResult<T>, String)>
    where
        F: FnOnce(&[u8]) -> io::Result<T>,
    {
        let delivery_id = format!("delivery-{}-{}", now_ms, next_id());
        let bytes =
            read_regular_file_bounded(artifact, crate::MAX_ARTIFACT_BYTES)?;
        let artifact_hash = hash_bytes(&bytes);
        let path_display = artifact.display().to_string();
        let report = self.latest_report(artifact)?;
        let hard_block = match &report {
            None => Some(BlockReason::MissingReport),
            Some(report)
                if expected_report_id.trim().is_empty()
                    || report.id != expected_report_id
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
                "override delivery rejected: report missing, empty, or artifact changed"
            );
            return Ok((
                DeliveryResult {
                    outcome: DeliveryOutcome {
                        delivery_id,
                        delivered: false,
                        blocked: Some(reason),
                        report_id,
                        override_used: false,
                        exemption_code: None,
                    },
                    value: None,
                },
                artifact_hash,
            ));
        }
        let report = report.expect("report checked above");
        let revision_mismatched = contract_revision_mismatched(
            report.contract_revision,
            expected_contract_revision,
        );
        let blocked = if !report.ok {
            Some(BlockReason::VerificationFailed)
        } else if revision_mismatched {
            Some(BlockReason::ContractRevisionMismatch)
        } else {
            None
        };
        let override_used = blocked.is_some();
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
            Self::append_json(
                &self.events_path(),
                &AuditEvent::DeliverOverride {
                    delivery_id: delivery_id.clone(),
                    report_id: report_id.clone().unwrap_or_default(),
                    artifact_path: path_display.clone(),
                    artifact_hash: artifact_hash.clone(),
                    code: reason.code().to_owned(),
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
        let value = match stage(&bytes) {
            Ok(value) => value,
            Err(error) => {
                if abort_on_stage_error {
                    let _ = Self::append_json(
                        &self.events_path(),
                        &AuditEvent::DeliverAborted {
                            delivery_id,
                            artifact_path: path_display,
                            artifact_hash,
                            reason: error.to_string(),
                            at_ms: now_ms,
                        },
                    );
                }
                return Err(error);
            }
        };
        Ok((
            DeliveryResult {
                outcome: DeliveryOutcome {
                    delivery_id,
                    delivered: false,
                    blocked,
                    report_id,
                    override_used,
                    exemption_code: None,
                },
                value: Some(value),
            },
            artifact_hash,
        ))
    }

    /// Explicit user override bound to one verified report and its exact file
    /// hash (§8.3). Refuses missing reports, empty or stale
    /// `expected_report_id`, and any file whose bytes changed since the report
    /// was recorded.
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
        let (mut result, artifact_hash) = self
            .stage_override_for_report_with_hash(
                artifact,
                expected_report_id,
                None,
                now_ms,
                false,
                send,
            )?;
        if result.value.is_some() {
            let path_display = artifact.display().to_string();
            self.confirm_delivered(
                &result.outcome.delivery_id,
                &path_display,
                &artifact_hash,
                now_ms,
            )?;
            result.outcome.delivered = true;
            tracing::info!(
                target: "flowy.delivery",
                delivery_id = %result.outcome.delivery_id,
                artifact_hash = %artifact_hash,
                outcome = "delivered",
                override_used = result.outcome.override_used,
                exemption_code = "",
                "delivery sent"
            );
        }
        Ok(result)
    }
}

fn store_lock(path: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
        OnceLock::new();
    let registry = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    registry
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(path.to_path_buf())
        .or_default()
        .clone()
}

fn ensure_trailing_newline(path: &Path) -> io::Result<()> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.len() == 0 {
        return Ok(());
    }
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(());
    }
    let mut handle = std::fs::OpenOptions::new().append(true).open(path)?;
    handle.write_all(b"\n")?;
    handle.flush()?;
    handle.sync_all()
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

    #[test]
    fn stage_failure_aborts_intent_and_override_is_scoped_by_report() {
        let root = scratch("stage-abort-and-scope");
        let store = DeliveryStore::new(root.join("delivery"));
        let path_a = root.join("sess-a").join("deck.pptx");
        let path_b = root.join("sess-b").join("deck.pptx");
        std::fs::create_dir_all(path_a.parent().unwrap()).unwrap();
        std::fs::create_dir_all(path_b.parent().unwrap()).unwrap();
        std::fs::write(&path_a, b"PK identical-bytes").unwrap();
        std::fs::write(&path_b, b"PK identical-bytes").unwrap();

        let rep_a = store
            .record_report(
                &path_a,
                vec![Issue {
                    code: "STRUCTURE".into(),
                    field: "slides".into(),
                }],
                1_000,
            )
            .unwrap();
        let rep_b = store
            .record_report(
                &path_b,
                vec![Issue {
                    code: "STRUCTURE".into(),
                    field: "slides".into(),
                }],
                1_100,
            )
            .unwrap();

        // 1) Local `stage_override_for_report` failure must abort the intent
        //    so no ghost in-doubt delivery remains.
        let failed =
            store.stage_override_for_report(&path_a, &rep_a.id, 1_200, |_| {
                Err::<(), _>(io::Error::other("local prepare failed"))
            });
        assert!(failed.is_err());
        assert!(
            store.in_doubt_deliveries().unwrap().is_empty(),
            "failed local staging must close DeliverIntent with DeliverAborted"
        );

        // 2) Overriding session A's report must NOT mark session B's report as
        //    overridden even when both files have identical SHA-256 bytes.
        let (_, send) = counting_sender();
        let delivered = store
            .deliver_override_for_report(&path_a, &rep_a.id, 1_300, send)
            .unwrap();
        assert!(delivered.outcome.delivered);
        assert!(
            store
                .report_was_overridden(
                    &rep_a.id,
                    &path_a.display().to_string(),
                    &rep_a.artifact_hash,
                )
                .unwrap()
        );
        assert!(
            !store
                .report_was_overridden(
                    &rep_b.id,
                    &path_b.display().to_string(),
                    &rep_b.artifact_hash,
                )
                .unwrap(),
            "session A override must not poison session B's failed card with same sha256"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stage_with_writes_intent_only_until_confirmed_idempotently() {
        let root = scratch("two-phase-stage");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK staged");
        store.record_report(&path, Vec::new(), 1_000).unwrap();
        let staged = store
            .stage_with(&path, false, 2_000, |bytes| Ok(hash_bytes(bytes)))
            .unwrap();
        assert!(!staged.outcome.delivered);
        assert!(staged.outcome.blocked.is_none());
        let hash = staged.value.expect("staged hash");
        let events_before =
            std::fs::read_to_string(store.events_path()).unwrap();
        assert!(events_before.contains("deliver_intent"));
        assert!(!events_before.contains("\"deliver\""));
        assert_eq!(store.in_doubt_deliveries().unwrap().len(), 1);

        assert_eq!(store.confirm_delivered_by_hash(&hash, 3_000).unwrap(), 1);
        assert_eq!(store.in_doubt_deliveries().unwrap().as_slice(), []);
        assert_eq!(store.confirm_delivered_by_hash(&hash, 4_000).unwrap(), 0);
        let events_after =
            std::fs::read_to_string(store.events_path()).unwrap();
        assert_eq!(events_after.matches("\"deliver\"").count(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_report_id_is_hard_blocked_on_override() {
        let root = scratch("override-empty-id");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK empty-id");
        store
            .record_report_bytes(
                &path,
                b"PK empty-id",
                vec![Issue {
                    code: "STRUCTURE".to_owned(),
                    field: "slides".to_owned(),
                }],
                1_000,
            )
            .unwrap();
        let (calls, send) = counting_sender();
        let rejected = store
            .stage_override_for_report(&path, "   ", 2_000, send)
            .unwrap();
        assert!(!rejected.outcome.delivered);
        assert_eq!(rejected.outcome.blocked, Some(BlockReason::HashMismatch));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn interior_corruption_in_reports_jsonl_fails_closed() {
        let root = scratch("corrupt-reports");
        let store = DeliveryStore::new(root.join("delivery"));
        let deck = artifact(&root, b"PK deck");
        let other = root.join("other.pptx");
        std::fs::write(&other, b"PK other").unwrap();

        // Passing report for `deck`, then a corrupted interior line (simulating
        // a damaged later failing report), then another valid report for `other`.
        store.record_report(&deck, Vec::new(), 1_000).unwrap();
        let mut handle = std::fs::OpenOptions::new()
            .append(true)
            .open(store.reports_path())
            .unwrap();
        writeln!(handle, r#"{{"id":"corrupt","ok":false"#).unwrap();
        drop(handle);
        store.record_report(&other, Vec::new(), 2_000).unwrap();

        assert_eq!(store.latest_report(&deck).unwrap(), None);
        assert_eq!(
            store.check(&deck).unwrap(),
            Some(BlockReason::MissingReport)
        );
        // A valid report recorded AFTER the corruption still succeeds.
        assert!(store.latest_report(&other).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stale_contract_revision_report_is_blocked_at_delivery_gate() {
        let root = scratch("stale-contract-rev");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK rev1-verified");
        let report = store
            .record_report_bytes_with_revision(
                &path,
                b"PK rev1-verified",
                Vec::new(),
                Some(1),
                1_000,
            )
            .unwrap();
        assert_eq!(report.contract_revision, Some(1));
        assert!(report.ok);
        // A rev=1 ok=true report passes for rev=1 and legacy None checks, but
        // is hard-blocked once the contract advances to rev=2.
        assert_eq!(store.check_for_revision(&path, Some(1)).unwrap(), None);
        assert_eq!(store.check(&path).unwrap(), None);
        let blocked = store.check_for_revision(&path, Some(2)).unwrap();
        assert_eq!(blocked, Some(BlockReason::ContractRevisionMismatch));
        assert_eq!(
            blocked.unwrap().code(),
            "DELIVERY_CONTRACT_REVISION_MISMATCH"
        );
        let (calls, send) = counting_sender();
        let staged = store
            .stage_with_revision(&path, false, Some(2), 2_000, send)
            .unwrap();
        assert!(!staged.outcome.delivered);
        assert_eq!(
            staged.outcome.blocked,
            Some(BlockReason::ContractRevisionMismatch)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let events = std::fs::read_to_string(store.events_path()).unwrap();
        assert!(events.contains("DELIVERY_CONTRACT_REVISION_MISMATCH"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stage_override_for_stale_ok_report_logs_blocked_and_override_audit() {
        let root = scratch("override-stale-rev");
        let store = DeliveryStore::new(root.join("delivery"));
        let path = artifact(&root, b"PK rev1-ok-override");
        let report = store
            .record_report_bytes_with_revision(
                &path,
                b"PK rev1-ok-override",
                Vec::new(),
                Some(1),
                1_000,
            )
            .unwrap();
        assert!(report.ok);

        let (calls, send) = counting_sender();
        let staged = store
            .stage_override_for_report_with_revision(
                &path,
                &report.id,
                Some(2),
                2_000,
                send,
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(staged.outcome.override_used);
        assert_eq!(
            staged.outcome.blocked,
            Some(BlockReason::ContractRevisionMismatch)
        );
        let events = std::fs::read_to_string(store.events_path()).unwrap();
        assert!(events.contains("\"deliver_blocked\""));
        assert!(events.contains("\"deliver_override\""));
        assert!(events.contains("DELIVERY_CONTRACT_REVISION_MISMATCH"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
