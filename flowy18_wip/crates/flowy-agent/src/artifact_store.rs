//! Durable snapshots for explicitly published conversation artifacts.
//!
//! Prepared snapshots are invisible until the engine has durably saved the
//! whole-turn `DeliveryCommit`. The independent marker is written last and is
//! the only source of list visibility.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use flowy_agent_core::{
    ArtifactReceipt, DeliveryCommit, DeliveryCommitter, SessionOrigin,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_DELIVERY_FILES: usize = 8;
pub const MAX_DELIVERY_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_STORE_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub const MAX_STORED_ITEMS: usize = 1_000;
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SCANNED_ENTRIES: usize = 10_000;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const PREPARED_RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const VIEW_COPY_RETENTION_MS: u64 = 3 * 24 * 60 * 60 * 1_000;
const MAX_RETAINED_VIEW_COPIES: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationState {
    Prepared,
    Committed,
    DeletePending,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub schema_version: u32,
    pub receipt: ArtifactReceipt,
    /// Source basename captured before an optional display name is applied.
    #[serde(default)]
    pub original_basename: String,
    /// Source path relative to the owning session workspace. Older manifests
    /// do not carry this field and are treated as having no reusable source.
    #[serde(default)]
    pub source_relative_path: String,
    /// True when the snapshot was inferred from a successful conversation
    /// write instead of an explicit host delivery. Superseded imports are safe
    /// to reclaim because they never own a chat artifact card.
    #[serde(default)]
    pub conversation_import: bool,
    /// True when a newer conversation import superseded this payload. Unlike a
    /// user tombstone, a superseded version may be captured again if its bytes
    /// return in a later turn.
    #[serde(default)]
    pub superseded: bool,
    pub origin: SessionOrigin,
    #[serde(default)]
    pub origin_title_snapshot: String,
    pub payload_relative_path: String,
    pub publication_state: PublicationState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactPreference {
    pub favorite: bool,
    pub trashed_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub manifest: ArtifactManifest,
    pub preference: ArtifactPreference,
    pub available: bool,
    /// The payload metadata cannot match the receipt or points at a link.
    /// Digest verification happens when the payload is read.
    pub corrupt: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewCopy {
    pub request_id: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtifactUsage {
    pub stored_bytes: u64,
    pub staging_bytes: u64,
    pub view_bytes: u64,
    pub pending_delete_bytes: u64,
    pub stored_items: usize,
    pub max_items: usize,
    pub max_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct PrepareArtifact {
    pub source_root: PathBuf,
    pub relative_path: String,
    /// Stable source identity used to reuse and supersede conversation
    /// snapshots. For `publish_file`, this is relative to the session
    /// workspace rather than the per-turn output directory.
    pub source_relative_path: String,
    /// See [`ArtifactManifest::conversation_import`].
    pub conversation_import: bool,
    pub session_id: String,
    pub submission_id: String,
    pub tool_call_id: String,
    pub delivery_key: String,
    pub display_name: Option<String>,
    pub origin: SessionOrigin,
}

#[derive(Clone)]
pub struct ArtifactStore {
    inner: Arc<StoreInner>,
}

struct StoreInner {
    root: PathBuf,
    serial: Mutex<()>,
    cleanup_diagnostics: Mutex<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeliveryMarker {
    schema_version: u32,
    commit: DeliveryCommit,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PreferencesFile {
    schema_version: u32,
    entries: BTreeMap<String, ArtifactPreference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ViewCopyMeta {
    request_id: String,
    artifact_id: String,
    file_name: String,
    sha256: String,
    created_at_ms: u64,
}

#[derive(Debug)]
struct Snapshot {
    digest: String,
    size: u64,
    prefix: Vec<u8>,
}

impl std::fmt::Debug for ArtifactStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtifactStore")
            .field("root", &self.inner.root)
            .finish_non_exhaustive()
    }
}

impl ArtifactStore {
    pub fn open(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        reject_link(&root)?;
        let root = fs::canonicalize(root)?;
        for name in [
            "staging",
            "items",
            "deliveries",
            "views",
            "views-meta",
            "session-deletions",
            "sources-deleted",
        ] {
            let dir = root.join(name);
            fs::create_dir_all(&dir)?;
            reject_link(&dir)?;
        }
        let store = Self {
            inner: Arc::new(StoreInner {
                root,
                serial: Mutex::new(()),
                cleanup_diagnostics: Mutex::new(Vec::new()),
            }),
        };
        if let Err(error) = store.retry_pending_deletions() {
            tracing::warn!(%error, "pending artifact deletion retained for retry");
        }
        if let Err(error) = store.prune_superseded_conversation_imports() {
            tracing::warn!(%error, "superseded conversation artifacts retained for retry");
        }
        if let Err(error) = store.cleanup_stale_view_copies() {
            tracing::warn!(%error, "stale view copies retained for retry");
        }
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn committed_ids(&self) -> io::Result<BTreeSet<String>> {
        let _guard = self.lock();
        self.committed_ids_locked()
    }

    /// Whether `(session_id, origin, source_relative_path, sha256)` already
    /// matches the latest committed (or tombstoned) snapshot for that source.
    pub(crate) fn is_source_already_committed(
        &self,
        session_id: &str,
        origin: &SessionOrigin,
        source_relative_path: &str,
        sha256: &str,
    ) -> io::Result<bool> {
        if source_relative_path.is_empty() || sha256.is_empty() {
            return Ok(false);
        }
        let _guard = self.lock();
        let initial = self.scan_locked()?;
        let committed = self.committed_ids_locked()?;
        let mut latest_rank: Option<(u64, String)> = None;
        for manifest in initial.manifests.iter().filter(|manifest| {
            manifest.receipt.session_id == session_id
                && manifest.source_relative_path == source_relative_path
                && committed.contains(&manifest.receipt.artifact_id)
                && matches!(
                    manifest.publication_state,
                    PublicationState::Committed
                        | PublicationState::DeletePending
                        | PublicationState::Deleted
                )
        }) {
            let rank = (
                manifest.receipt.created_at_ms,
                manifest.receipt.artifact_id.clone(),
            );
            if latest_rank.as_ref().is_none_or(|current| rank > *current) {
                latest_rank = Some(rank);
            }
        }
        let Some(latest_rank) = latest_rank else {
            return Ok(false);
        };
        Ok(initial.manifests.iter().any(|manifest| {
            let rank = (
                manifest.receipt.created_at_ms,
                manifest.receipt.artifact_id.clone(),
            );
            manifest.receipt.session_id == session_id
                && &manifest.origin == origin
                && manifest.source_relative_path == source_relative_path
                && manifest.receipt.sha256 == sha256
                && matches!(
                    manifest.publication_state,
                    PublicationState::Committed
                        | PublicationState::DeletePending
                        | PublicationState::Deleted
                )
                && !manifest.superseded
                && committed.contains(&manifest.receipt.artifact_id)
                && rank == latest_rank
        }))
    }

    pub fn diagnostics(&self) -> io::Result<Vec<String>> {
        let _guard = self.lock();
        let mut diagnostics =
            self.scan_with_diagnostics_locked(false)?.diagnostics;
        diagnostics.extend(
            self.inner
                .cleanup_diagnostics
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        );
        Ok(diagnostics)
    }

    pub fn usage(&self) -> io::Result<ArtifactUsage> {
        let _guard = self.lock();
        let scan = self.scan_with_diagnostics_locked(false)?;
        let pending_delete_bytes = scan
            .manifests
            .iter()
            .filter(|manifest| {
                matches!(
                    manifest.publication_state,
                    PublicationState::DeletePending
                )
            })
            .filter_map(|manifest| {
                fs::metadata(self.payload_path(&manifest.receipt.artifact_id))
                    .ok()
            })
            .fold(0_u64, |total, metadata| {
                total.saturating_add(metadata.len())
            });
        Ok(ArtifactUsage {
            stored_bytes: scan.stored_bytes,
            staging_bytes: scan.staging_bytes,
            view_bytes: scan.view_bytes,
            pending_delete_bytes,
            stored_items: scan.payload_files,
            max_items: MAX_STORED_ITEMS,
            max_bytes: MAX_STORE_BYTES,
        })
    }

    pub fn pending_views(&self) -> io::Result<Vec<ViewCopy>> {
        let _guard = self.lock();
        let scan = self.scan_with_diagnostics_locked(false)?;
        if scan.entry_count > MAX_SCANNED_ENTRIES {
            return Err(invalid("view scan limit exceeded"));
        }
        let mut copies = Vec::new();
        for entry in fs::read_dir(self.inner.root.join("views"))? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let request_id = entry.file_name().to_string_lossy().to_string();
            validate_identifier(&request_id)?;
            for child in fs::read_dir(entry.path())? {
                let child = child?;
                if child.file_type()?.is_file() {
                    copies.push(ViewCopy {
                        request_id: request_id.clone(),
                        path: child.path(),
                    });
                }
            }
        }
        Ok(copies)
    }

    pub fn retry_pending_deletions(&self) -> io::Result<()> {
        let pending = {
            let _guard = self.lock();
            self.scan_with_diagnostics_locked(false)?
                .manifests
                .into_iter()
                .filter(|manifest| {
                    matches!(
                        manifest.publication_state,
                        PublicationState::DeletePending
                    )
                })
                .map(|manifest| manifest.receipt.artifact_id)
                .collect::<Vec<_>>()
        };
        let mut failure = None;
        for artifact_id in pending {
            if let Err(error) = self.permanently_delete(&artifact_id) {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn prune_superseded_conversation_imports(&self) -> io::Result<()> {
        let _guard = self.lock();
        self.prune_superseded_conversation_imports_locked()
    }

    fn prune_superseded_conversation_imports_locked(&self) -> io::Result<()> {
        let scan = self.scan_locked()?;
        let committed = self.committed_ids_locked()?;
        let mut latest = BTreeMap::new();
        for manifest in &scan.manifests {
            if manifest.source_relative_path.is_empty()
                || !committed.contains(&manifest.receipt.artifact_id)
                || !matches!(
                    manifest.publication_state,
                    PublicationState::Committed
                        | PublicationState::DeletePending
                        | PublicationState::Deleted
                )
            {
                continue;
            }
            let key = (
                manifest.receipt.session_id.clone(),
                manifest.source_relative_path.clone(),
            );
            let rank = (
                manifest.receipt.created_at_ms,
                manifest.receipt.artifact_id.clone(),
            );
            latest
                .entry(key)
                .and_modify(|current: &mut (u64, String)| {
                    if rank > *current {
                        current.clone_from(&rank);
                    }
                })
                .or_insert(rank);
        }

        let mut failure = None;
        for manifest in scan.manifests {
            if !manifest.conversation_import
                || !committed.contains(&manifest.receipt.artifact_id)
                || !matches!(
                    manifest.publication_state,
                    PublicationState::Committed
                        | PublicationState::DeletePending
                )
            {
                continue;
            }
            let key = (
                manifest.receipt.session_id.clone(),
                manifest.source_relative_path.clone(),
            );
            let rank = (
                manifest.receipt.created_at_ms,
                manifest.receipt.artifact_id.clone(),
            );
            if latest.get(&key).is_some_and(|current| *current == rank) {
                continue;
            }
            if let Err(error) =
                self.retire_conversation_import_locked(&manifest)
            {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn retire_conversation_import_locked(
        &self,
        manifest: &ArtifactManifest,
    ) -> io::Result<()> {
        let path = self
            .item_dir(&manifest.receipt.artifact_id)
            .join("manifest.json");
        let mut current = read_manifest(&path)?;
        if !current.conversation_import
            || current.receipt.artifact_id != manifest.receipt.artifact_id
        {
            return Ok(());
        }
        match current.publication_state {
            PublicationState::Deleted => return Ok(()),
            PublicationState::Prepared => {
                return Err(invalid(
                    "a prepared conversation import cannot be superseded",
                ));
            }
            PublicationState::Committed => {
                current.publication_state = PublicationState::DeletePending;
                current.superseded = true;
                self.write_metadata_locked(&path, &current)?;
            }
            PublicationState::DeletePending => {}
        }
        match fs::remove_file(self.payload_path(&current.receipt.artifact_id)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        current.publication_state = PublicationState::Deleted;
        self.write_metadata_locked(&path, &current)
    }

    /// Reconcile only successful durable per-turn commits. Prepared files
    /// without such a proof remain hidden and are never guessed into a result.
    pub fn recover_commits(
        &self,
        commits: &[DeliveryCommit],
    ) -> io::Result<()> {
        for commit in commits {
            if commit.completed_successfully {
                self.commit_delivery(commit)?;
            }
        }
        Ok(())
    }

    /// Commit one host-derived conversation snapshot synchronously. The
    /// caller must persist the matching history commit first.
    pub(crate) fn commit_prepared(
        &self,
        commit: &DeliveryCommit,
    ) -> io::Result<()> {
        self.commit_delivery(commit)
    }

    /// Keep a bounded, UI-visible diagnosis for a file that could not be
    /// imported without failing the rest of the conversation backfill.
    pub(crate) fn record_import_diagnostic(&self, message: String) {
        tracing::warn!(target: "flowy.storage", %message, "artifact import skipped");
        let mut diagnostics = self
            .inner
            .cleanup_diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if diagnostics.len() < 64 {
            diagnostics.push(message);
        }
    }

    /// Run only during host startup after all successful history commits have
    /// been recovered and before any new turn is allowed to start. Unreadable
    /// history never authorizes cleanup, even for an apparently orphaned file.
    pub fn cleanup_uncommitted(
        &self,
        history_commits: &[DeliveryCommit],
        history_readable: bool,
    ) -> io::Result<()> {
        if !history_readable {
            return Ok(());
        }
        let _guard = self.lock();
        // Preflight the complete controlled tree before removing anything.
        // Unknown/corrupt manifests and bounded-scan failures make the entire
        // cleanup pass fail closed, rather than accepting a partial proof set.
        self.scan_locked()?;
        let mut protected = BTreeSet::new();
        for commit in history_commits
            .iter()
            .filter(|commit| commit.completed_successfully)
        {
            validate_identifier(&commit.session_id)?;
            validate_identifier(&commit.submission_id)?;
            for artifact_id in &commit.artifact_ids {
                validate_identifier(artifact_id)?;
                protected.insert(artifact_id.clone());
            }
        }
        for entry in fs::read_dir(self.inner.root.join("deliveries"))? {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str())
                != Some("json")
            {
                continue;
            }
            let marker = self.read_delivery_marker(&entry.path())?;
            for artifact_id in marker.commit.artifact_ids {
                validate_identifier(&artifact_id)?;
                protected.insert(artifact_id);
            }
        }

        let staging = self.inner.root.join("staging");
        let directories = self.staged_artifact_directories_locked()?;
        let now = now_ms();
        let mut candidates = Vec::new();
        for directory in directories {
            let artifact_id = directory
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    invalid("invalid staging artifact identifier")
                })?;
            let prepared_path = directory.join("prepared.json");
            for child in fs::read_dir(&directory)? {
                let child = child?;
                let metadata = fs::symlink_metadata(child.path())?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || is_windows_reparse_point(&metadata)
                {
                    return Err(invalid(
                        "unknown staging layout blocks cleanup",
                    ));
                }
            }
            let old_enough = if prepared_path.exists() {
                let manifest = read_manifest(&prepared_path)?;
                if !matches!(
                    manifest.publication_state,
                    PublicationState::Prepared
                ) || self.staging_dir(
                    &manifest.receipt.session_id,
                    &manifest.receipt.submission_id,
                    &manifest.receipt.artifact_id,
                ) != directory
                {
                    return Err(invalid(
                        "staging manifest identity does not match its path",
                    ));
                }
                now.saturating_sub(manifest.receipt.created_at_ms)
                    >= PREPARED_RETENTION_MS
            } else {
                if directory.join("manifest.json").exists() {
                    return Err(invalid(
                        "unexpected final manifest inside staging",
                    ));
                }
                // A raw incomplete copy has no published receipt. Startup has
                // no active copier, and successful proofs below still protect
                // its ID in case a crash interrupted the last metadata write.
                true
            };
            if old_enough && !protected.contains(artifact_id) {
                candidates.push(directory);
            }
        }

        let mut failure = None;
        for directory in candidates {
            reject_link(&directory)?;
            if !fs::canonicalize(&directory)?.starts_with(&staging) {
                return Err(invalid(
                    "staging cleanup target escapes its managed root",
                ));
            }
            match fs::remove_dir_all(&directory) {
                Ok(()) => {
                    let submission = directory
                        .parent()
                        .expect("staging artifact has parent");
                    let session = submission
                        .parent()
                        .expect("staging submission has parent");
                    for parent in [submission, session] {
                        match fs::remove_dir(parent) {
                            Ok(()) => {}
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    io::ErrorKind::NotFound
                                        | io::ErrorKind::DirectoryNotEmpty
                                ) => {}
                            Err(error) => {
                                tracing::warn!(path = %parent.display(), %error, "empty artifact staging directory retained for cleanup retry");
                                failure = Some(error);
                            }
                        }
                    }
                }
                Err(error) => {
                    // Occupied Windows files remain charged by the next actual
                    // filesystem scan. No speculative quota is released.
                    tracing::warn!(path = %directory.display(), %error, "uncommitted artifact retained for cleanup retry");
                    failure = Some(error);
                }
            }
        }
        let mut diagnostics = self
            .inner
            .cleanup_diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        diagnostics.clear();
        if let Some(error) = &failure {
            diagnostics.push(format!(
                "Uncommitted artifact cleanup is pending: {error}"
            ));
        }
        drop(diagnostics);
        failure.map_or(Ok(()), Err)
    }

    fn staged_artifact_directories_locked(&self) -> io::Result<Vec<PathBuf>> {
        let mut directories = vec![self.inner.root.join("staging")];
        let mut count = 0_usize;
        for _ in 0..3 {
            let mut next = Vec::new();
            for directory in directories {
                for entry in fs::read_dir(directory)? {
                    let entry = entry?;
                    count = count.saturating_add(1);
                    if count > MAX_SCANNED_ENTRIES {
                        return Err(invalid("staging scan limit exceeded"));
                    }
                    let metadata = fs::symlink_metadata(entry.path())?;
                    if !metadata.is_dir()
                        || metadata.file_type().is_symlink()
                        || is_windows_reparse_point(&metadata)
                    {
                        return Err(invalid(
                            "unknown staging directory layout",
                        ));
                    }
                    let name = entry.file_name();
                    validate_identifier(name.to_str().ok_or_else(|| {
                        invalid("invalid staging identifier")
                    })?)?;
                    next.push(entry.path());
                }
            }
            directories = next;
        }
        Ok(directories)
    }

    pub fn source_deleted(&self, session_id: &str) -> io::Result<bool> {
        validate_identifier(session_id)?;
        let path = self
            .inner
            .root
            .join("sources-deleted")
            .join(format!("{session_id}.json"));
        if !path.exists() {
            return Ok(false);
        }
        let marker =
            read_limited_json::<SourceDeletion>(&path, MAX_MANIFEST_BYTES)?;
        Ok(marker.schema_version == 1 && marker.session_id == session_id)
    }

    pub(crate) fn source_deleted_sessions(&self) -> io::Result<Vec<String>> {
        let _guard = self.lock();
        let dir = self.inner.root.join("sources-deleted");
        let mut sessions = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_symlink()
                || !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|value| value.to_str())
                    != Some("json")
            {
                continue;
            }
            let session_id = entry
                .path()
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| invalid("invalid source deletion marker name"))?
                .to_owned();
            validate_identifier(&session_id)?;
            let marker = read_limited_json::<SourceDeletion>(
                &entry.path(),
                MAX_MANIFEST_BYTES,
            )?;
            if marker.schema_version == 1 && marker.session_id == session_id {
                sessions.push(session_id);
            }
        }
        sessions.sort();
        Ok(sessions)
    }

    pub(crate) fn clear_source_deleted(
        &self,
        session_id: &str,
    ) -> io::Result<()> {
        validate_identifier(session_id)?;
        let _guard = self.lock();
        remove_file_if_exists(&self.source_deletion_path(session_id))
    }

    pub(crate) fn session_deletions(
        &self,
    ) -> io::Result<Vec<SessionDeletionPlan>> {
        let _guard = self.lock();
        let dir = self.inner.root.join("session-deletions");
        let mut plans = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_symlink()
                || entry.file_type()?.is_dir()
                || path.extension().and_then(|value| value.to_str())
                    != Some("json")
            {
                continue;
            }
            let session_id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    invalid("invalid session deletion marker name")
                })?;
            plans.push(read_session_deletion_plan(&path, session_id)?);
        }
        plans.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        Ok(plans)
    }

    pub(crate) fn has_active_deletion_plan(&self, session_id: &str) -> bool {
        validate_identifier(session_id).is_ok()
            && self.session_deletion_path(session_id).exists()
    }

    pub(crate) fn prepare_session_deletion(
        &self,
        session_id: &str,
        artifact_ids: &[String],
        submission_ids: &[String],
    ) -> io::Result<()> {
        self.prepare_cascade_session_deletion(
            session_id,
            &[],
            artifact_ids,
            submission_ids,
        )
    }

    pub(crate) fn prepare_cascade_session_deletion(
        &self,
        session_id: &str,
        child_session_ids: &[String],
        artifact_ids: &[String],
        submission_ids: &[String],
    ) -> io::Result<()> {
        validate_identifier(session_id)?;
        let mut children = child_session_ids
            .iter()
            .filter(|cid| cid.as_str() != session_id)
            .cloned()
            .collect::<BTreeSet<String>>();
        for child_id in &children {
            validate_identifier(child_id)?;
        }
        let mut all_sessions = BTreeSet::new();
        all_sessions.insert(session_id.to_owned());
        all_sessions.extend(children.iter().cloned());
        let mut artifacts =
            artifact_ids.iter().cloned().collect::<BTreeSet<String>>();
        let mut submissions =
            submission_ids.iter().cloned().collect::<BTreeSet<String>>();
        for artifact_id in &artifacts {
            validate_identifier(artifact_id)?;
        }
        for submission_id in &submissions {
            validate_identifier(submission_id)?;
        }

        let _guard = self.lock();
        let scan = self.scan_with_diagnostics_locked(false)?;
        for manifest in scan.manifests {
            if all_sessions.contains(&manifest.receipt.session_id) {
                artifacts.insert(manifest.receipt.artifact_id);
            }
        }
        for entry in fs::read_dir(self.inner.root.join("deliveries"))? {
            let entry = entry?;
            if entry.file_type()?.is_symlink()
                || !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|value| value.to_str())
                    != Some("json")
            {
                continue;
            }
            let Ok(marker) = self.read_delivery_marker(&entry.path()) else {
                continue;
            };
            if all_sessions.contains(&marker.commit.session_id) {
                submissions.insert(marker.commit.submission_id);
                artifacts.extend(marker.commit.artifact_ids);
            }
        }

        let path = self.session_deletion_path(session_id);
        if path.exists() {
            let mut plan = read_session_deletion_plan(&path, session_id)?;
            if plan.state == SessionDeletionState::Committed {
                return Ok(());
            }
            let before = (
                plan.artifact_ids.clone(),
                plan.submission_ids.clone(),
                plan.child_session_ids.clone(),
            );
            plan.artifact_ids.extend(artifacts);
            plan.submission_ids.extend(submissions);
            plan.child_session_ids.extend(std::mem::take(&mut children));
            plan.artifact_ids.sort();
            plan.artifact_ids.dedup();
            plan.submission_ids.sort();
            plan.submission_ids.dedup();
            plan.child_session_ids.sort();
            plan.child_session_ids.dedup();
            if before
                != (
                    plan.artifact_ids.clone(),
                    plan.submission_ids.clone(),
                    plan.child_session_ids.clone(),
                )
            {
                self.write_metadata_locked(&path, &plan)?;
            }
            return Ok(());
        }

        let plan = SessionDeletionPlan {
            schema_version: 1,
            session_id: session_id.to_owned(),
            state: SessionDeletionState::Prepared,
            artifact_ids: artifacts.into_iter().collect(),
            submission_ids: submissions.into_iter().collect(),
            child_session_ids: children.into_iter().collect(),
            created_at_ms: now_ms(),
        };
        self.write_metadata_locked(&path, &plan)
    }

    pub(crate) fn cancel_session_deletion(
        &self,
        session_id: &str,
    ) -> io::Result<()> {
        validate_identifier(session_id)?;
        let _guard = self.lock();
        let path = self.session_deletion_path(session_id);
        if !path.exists() {
            return Ok(());
        }
        let plan = read_session_deletion_plan(&path, session_id)?;
        if plan.state == SessionDeletionState::Committed {
            return Err(invalid(
                "a committed session deletion cannot be rolled back",
            ));
        }
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn commit_session_deletion(
        &self,
        session_id: &str,
    ) -> io::Result<()> {
        validate_identifier(session_id)?;
        let _guard = self.lock();
        let path = self.session_deletion_path(session_id);
        let mut plan = read_session_deletion_plan(&path, session_id)?;
        if plan.state == SessionDeletionState::Committed {
            return Ok(());
        }
        plan.state = SessionDeletionState::Committed;
        self.write_metadata_locked(&path, &plan)
    }

    pub(crate) fn cleanup_session_deletion(
        &self,
        session_id: &str,
    ) -> io::Result<()> {
        validate_identifier(session_id)?;
        let _guard = self.lock();
        let path = self.session_deletion_path(session_id);
        let plan = read_session_deletion_plan(&path, session_id)?;
        if plan.state != SessionDeletionState::Committed {
            return Err(invalid("session deletion is not committed"));
        }
        let mut all_sessions = BTreeSet::new();
        all_sessions.insert(session_id.to_owned());
        for child_id in &plan.child_session_ids {
            validate_identifier(child_id)?;
            all_sessions.insert(child_id.clone());
        }
        let mut artifacts =
            plan.artifact_ids.into_iter().collect::<BTreeSet<String>>();
        let mut submissions = plan
            .submission_ids
            .into_iter()
            .collect::<BTreeSet<String>>();
        let scan = self.scan_with_diagnostics_locked(false)?;
        for manifest in scan.manifests {
            if all_sessions.contains(&manifest.receipt.session_id) {
                artifacts.insert(manifest.receipt.artifact_id);
            }
        }
        for entry in fs::read_dir(self.inner.root.join("deliveries"))? {
            let entry = entry?;
            if entry.file_type()?.is_symlink()
                || !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|value| value.to_str())
                    != Some("json")
            {
                continue;
            }
            let Ok(marker) = self.read_delivery_marker(&entry.path()) else {
                continue;
            };
            if all_sessions.contains(&marker.commit.session_id) {
                submissions.insert(marker.commit.submission_id);
                artifacts.extend(marker.commit.artifact_ids);
            }
        }

        for artifact_id in &artifacts {
            validate_identifier(artifact_id)?;
            Self::remove_managed_dir(&self.item_dir(artifact_id))?;
        }
        for sid in &all_sessions {
            Self::remove_managed_dir(
                &self.inner.root.join("staging").join(sid),
            )?;
            for submission_id in &submissions {
                validate_identifier(submission_id)?;
                remove_file_if_exists(&self.delivery_path(sid, submission_id))?;
            }
        }

        let mut preferences = self.preferences_locked()?;
        let before = preferences.len();
        preferences.retain(|artifact_id, _| !artifacts.contains(artifact_id));
        if preferences.len() != before {
            self.save_preferences_locked(&preferences)?;
        }
        Ok(())
    }

    pub(crate) fn complete_session_deletion(
        &self,
        session_id: &str,
    ) -> io::Result<()> {
        validate_identifier(session_id)?;
        let _guard = self.lock();
        let path = self.session_deletion_path(session_id);
        if !path.exists() {
            return Ok(());
        }
        let plan = read_session_deletion_plan(&path, session_id)?;
        if plan.state != SessionDeletionState::Committed {
            return Err(invalid("session deletion is not committed"));
        }
        remove_file_if_exists(&self.source_deletion_path(session_id))?;
        remove_file_if_exists(&path)
    }

    fn session_deletion_ids_locked(&self) -> io::Result<BTreeSet<String>> {
        let dir = self.inner.root.join("session-deletions");
        let mut ids = BTreeSet::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_symlink()
                || !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|value| value.to_str())
                    != Some("json")
            {
                continue;
            }
            let session_id = entry
                .path()
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| invalid("invalid session deletion marker name"))?
                .to_owned();
            validate_identifier(&session_id)?;
            ids.insert(session_id);
        }
        Ok(ids)
    }

    fn remove_managed_dir(path: &Path) -> io::Result<()> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        reject_link(path)?;
        if !metadata.is_dir() {
            return Err(invalid(
                "managed artifact deletion target is not a directory",
            ));
        }
        fs::remove_dir_all(path)
    }

    /// Validate and snapshot one model-selected file under a host-owned output root.
    /// The model never supplies a managed-store path or artifact ID.
    pub fn prepare(
        &self,
        request: PrepareArtifact,
    ) -> io::Result<ArtifactReceipt> {
        validate_identifier(&request.session_id)?;
        validate_identifier(&request.submission_id)?;
        if request.tool_call_id.is_empty()
            || request.tool_call_id.len() > 1024
            || request.tool_call_id.chars().any(char::is_control)
        {
            return Err(invalid("invalid host tool call identifier"));
        }
        validate_delivery_key(&request.delivery_key)?;
        let relative = validate_relative_path(&request.relative_path)?;
        let source_relative_path = request.source_relative_path.clone();
        let conversation_import = request.conversation_import;
        let source = request.source_root.join(&relative);
        let mut pinned =
            crate::artifact_source::open(&request.source_root, &relative)?;
        let input = &mut pinned.file;
        let opened_meta = input.metadata()?;
        if !opened_meta.is_file() || opened_meta.len() == 0 {
            return Err(invalid(
                "published path is not a non-empty regular file",
            ));
        }
        if opened_meta.len() > MAX_ARTIFACT_BYTES {
            return Err(invalid("artifact exceeds the 128 MiB per-file limit"));
        }
        let _guard = self.lock();
        let initial = self.scan_locked()?;
        let snapshot = snapshot_file(input)?;
        if snapshot.size == 0 || snapshot.size > MAX_ARTIFACT_BYTES {
            return Err(invalid(
                "artifact size is outside the supported range",
            ));
        }
        let after = input.metadata()?;
        if after.len() != opened_meta.len()
            || after.modified().ok() != opened_meta.modified().ok()
            || snapshot.size != after.len()
        {
            return Err(invalid(
                "source file changed while it was being copied",
            ));
        }
        validate_known_header(&request.relative_path, &snapshot.prefix)?;

        // A conversation file has one stable source identity. Reuse an
        // already committed snapshot when both the source identity and bytes
        // match, regardless of which submission originally captured it.
        if !source_relative_path.is_empty() {
            let committed = self.committed_ids_locked()?;
            let mut latest_source = BTreeMap::new();
            for manifest in initial.manifests.iter().filter(|manifest| {
                manifest.receipt.session_id == request.session_id
                    && manifest.source_relative_path == source_relative_path
                    && committed.contains(&manifest.receipt.artifact_id)
                    && matches!(
                        manifest.publication_state,
                        PublicationState::Committed
                            | PublicationState::DeletePending
                            | PublicationState::Deleted
                    )
            }) {
                let rank = (
                    manifest.receipt.created_at_ms,
                    manifest.receipt.artifact_id.clone(),
                );
                latest_source
                    .entry((
                        manifest.receipt.session_id.clone(),
                        manifest.source_relative_path.clone(),
                    ))
                    .and_modify(|current: &mut (u64, String)| {
                        if rank > *current {
                            current.clone_from(&rank);
                        }
                    })
                    .or_insert(rank);
            }
            if let Some(existing) = initial.manifests.iter().find(|manifest| {
                let rank = (
                    manifest.receipt.created_at_ms,
                    manifest.receipt.artifact_id.clone(),
                );
                manifest.receipt.session_id == request.session_id
                    && manifest.origin == request.origin
                    && manifest.source_relative_path == source_relative_path
                    && manifest.receipt.sha256 == snapshot.digest
                    && manifest.receipt.size_bytes == snapshot.size
                    && matches!(
                        manifest.publication_state,
                        PublicationState::Committed
                            | PublicationState::DeletePending
                            | PublicationState::Deleted
                    )
                    && (!manifest.conversation_import || conversation_import)
                    && !manifest.superseded
                    && committed.contains(&manifest.receipt.artifact_id)
                    && latest_source
                        .get(&(
                            manifest.receipt.session_id.clone(),
                            manifest.source_relative_path.clone(),
                        ))
                        .is_some_and(|current| *current == rank)
            }) {
                return Ok(existing.receipt.clone());
            }
        }

        if let Some(existing) = initial.manifests.iter().find(|manifest| {
            manifest.receipt.session_id == request.session_id
                && manifest.receipt.submission_id == request.submission_id
                && manifest.receipt.delivery_key == request.delivery_key
        }) {
            if existing.receipt.sha256 != snapshot.digest
                || existing.receipt.size_bytes != snapshot.size
            {
                return Err(invalid(
                    "delivery key already names different file contents",
                ));
            }
            if matches!(existing.publication_state, PublicationState::Deleted) {
                return Err(invalid("this delivery was permanently removed"));
            }
            let mut receipt = existing.receipt.clone();
            receipt.tool_call_id = request.tool_call_id;
            return Ok(receipt);
        }

        let turn_items = initial
            .manifests
            .iter()
            .filter(|manifest| {
                manifest.receipt.session_id == request.session_id
                    && manifest.receipt.submission_id == request.submission_id
                    && !matches!(
                        manifest.publication_state,
                        PublicationState::Deleted
                    )
            })
            .collect::<Vec<_>>();
        let turn_bytes = turn_items.iter().fold(0_u64, |sum, item| {
            sum.saturating_add(item.receipt.size_bytes)
        });
        if turn_items.len() >= MAX_DELIVERY_FILES
            || turn_bytes.saturating_add(snapshot.size) > MAX_DELIVERY_BYTES
        {
            return Err(invalid(
                "delivery exceeds the per-turn file or size limit",
            ));
        }
        let live_items = initial.payload_files;
        if live_items >= MAX_STORED_ITEMS
            || initial.payload_bytes.saturating_add(snapshot.size)
                > MAX_STORE_BYTES
        {
            return Err(invalid("artifact library storage limit reached"));
        }
        if initial
            .metadata_bytes
            .saturating_add(MAX_MANIFEST_BYTES * 2)
            > MAX_METADATA_BYTES
            || initial.entry_count.saturating_add(6) > MAX_SCANNED_ENTRIES
        {
            return Err(invalid(
                "artifact metadata or scan-entry limit reached",
            ));
        }

        let artifact_id = random_id()?;
        let original_basename = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_owned();
        let file_name = safe_display_name(
            request.display_name.as_deref(),
            &original_basename,
        )?;
        let created_at_ms = initial
            .manifests
            .iter()
            .filter(|manifest| {
                manifest.receipt.session_id == request.session_id
                    && !source_relative_path.is_empty()
                    && manifest.source_relative_path == source_relative_path
            })
            .map(|manifest| manifest.receipt.created_at_ms)
            .max()
            .map_or_else(now_ms, |previous| {
                now_ms().max(previous.saturating_add(1))
            });
        let receipt = ArtifactReceipt {
            artifact_id: artifact_id.clone(),
            delivery_key: request.delivery_key,
            session_id: request.session_id.clone(),
            submission_id: request.submission_id.clone(),
            tool_call_id: request.tool_call_id,
            display_name: file_name,
            media_type: media_type(&source),
            size_bytes: snapshot.size,
            sha256: snapshot.digest,
            created_at_ms,
            origin_title_snapshot: String::new(),
        };
        let staging = self.staging_dir(
            &request.session_id,
            &request.submission_id,
            &artifact_id,
        );
        fs::create_dir_all(&staging)?;
        let payload = staging.join("payload");
        let mut staged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&payload)?;
        let copied = copy_and_hash(input, &mut staged)?;
        let after_copy = input.metadata()?;
        if copied.digest != receipt.sha256 || copied.size != receipt.size_bytes
        {
            let _ = fs::remove_dir_all(&staging);
            return Err(invalid(
                "source file changed before the snapshot was staged",
            ));
        }
        if after_copy.len() != opened_meta.len()
            || after_copy.modified().ok() != opened_meta.modified().ok()
        {
            let _ = fs::remove_dir_all(&staging);
            return Err(invalid(
                "source file changed while staging the snapshot",
            ));
        }
        staged.sync_all()?;
        let manifest = ArtifactManifest {
            schema_version: 1,
            receipt: receipt.clone(),
            original_basename,
            source_relative_path,
            conversation_import,
            superseded: false,
            origin: request.origin,
            origin_title_snapshot: String::new(),
            payload_relative_path: format!("items/{artifact_id}/payload"),
            publication_state: PublicationState::Prepared,
        };
        let bytes = serde_json::to_vec(&manifest).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            let _ = fs::remove_dir_all(&staging);
            return Err(invalid(
                "artifact manifest exceeds the metadata limit",
            ));
        }
        write_new_synced(&staging.join("prepared.json"), &bytes)?;
        Ok(receipt)
    }

    pub fn list(&self) -> io::Result<Vec<ArtifactRecord>> {
        let _guard = self.lock();
        self.list_locked(false)
    }

    pub fn list_with_tombstones(&self) -> io::Result<Vec<ArtifactRecord>> {
        let _guard = self.lock();
        self.list_locked(true)
    }

    fn list_locked(
        &self,
        include_deleted: bool,
    ) -> io::Result<Vec<ArtifactRecord>> {
        let scan = self.scan_with_diagnostics_locked(false)?;
        let committed = self.committed_ids_locked()?;
        let deleting = self.session_deletion_ids_locked()?;
        let preferences = self.preferences_locked()?;
        let mut latest_sources = BTreeMap::new();
        if !include_deleted {
            for manifest in scan.manifests.iter().filter(|manifest| {
                !manifest.source_relative_path.is_empty()
                    && !deleting.contains(&manifest.receipt.session_id)
                    && committed.contains(&manifest.receipt.artifact_id)
                    && matches!(
                        manifest.publication_state,
                        PublicationState::Committed
                            | PublicationState::DeletePending
                            | PublicationState::Deleted
                    )
            }) {
                let key = (
                    manifest.receipt.session_id.clone(),
                    manifest.source_relative_path.clone(),
                );
                let rank = (
                    manifest.receipt.created_at_ms,
                    manifest.receipt.artifact_id.clone(),
                );
                latest_sources
                    .entry(key)
                    .and_modify(|current| {
                        if rank > *current {
                            *current = rank.clone();
                        }
                    })
                    .or_insert(rank);
            }
        }
        Ok(scan
            .manifests
            .into_iter()
            .filter(|manifest| {
                !deleting.contains(&manifest.receipt.session_id)
                    && (matches!(
                        manifest.publication_state,
                        PublicationState::Committed
                            | PublicationState::DeletePending
                    ) || (include_deleted
                        && matches!(
                            manifest.publication_state,
                            PublicationState::Deleted
                        )))
                    && committed.contains(&manifest.receipt.artifact_id)
                    && (include_deleted
                        || manifest.source_relative_path.is_empty()
                        || latest_sources
                            .get(&(
                                manifest.receipt.session_id.clone(),
                                manifest.source_relative_path.clone(),
                            ))
                            .is_some_and(|latest| {
                                *latest
                                    == (
                                        manifest.receipt.created_at_ms,
                                        manifest.receipt.artifact_id.clone(),
                                    )
                            }))
            })
            .map(|manifest| {
                let artifact_id = manifest.receipt.artifact_id.clone();
                let (available, corrupt) = self.payload_status(&manifest);
                ArtifactRecord {
                    manifest,
                    preference: preferences
                        .get(&artifact_id)
                        .cloned()
                        .unwrap_or_default(),
                    available,
                    corrupt,
                }
            })
            .collect())
    }

    /// Cheap list-time status. Listing must not read every payload; the full
    /// digest is verified on demand by `read_payload` and preview requests.
    fn payload_status(&self, manifest: &ArtifactManifest) -> (bool, bool) {
        if !matches!(manifest.publication_state, PublicationState::Committed) {
            return (false, false);
        }
        let path = self.payload_path(&manifest.receipt.artifact_id);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return (false, false);
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || is_windows_reparse_point(&metadata)
            || metadata.len() != manifest.receipt.size_bytes
        {
            return (false, true);
        }
        (true, false)
    }

    pub fn get(&self, artifact_id: &str) -> io::Result<Option<ArtifactRecord>> {
        validate_identifier(artifact_id)?;
        let _guard = self.lock();
        let manifest_path = self.item_dir(artifact_id).join("manifest.json");
        let manifest = match read_manifest(&manifest_path) {
            Ok(manifest) => manifest,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if manifest.receipt.artifact_id != artifact_id
            || !matches!(
                manifest.publication_state,
                PublicationState::Committed
                    | PublicationState::DeletePending
                    | PublicationState::Deleted
            )
        {
            return Ok(None);
        }
        if self
            .session_deletion_ids_locked()?
            .contains(&manifest.receipt.session_id)
        {
            return Ok(None);
        }
        let marker_path = self.delivery_path(
            &manifest.receipt.session_id,
            &manifest.receipt.submission_id,
        );
        let committed = match self.read_delivery_marker(&marker_path) {
            Ok(marker) => {
                marker.commit.completed_successfully
                    && marker.commit.session_id == manifest.receipt.session_id
                    && marker.commit.submission_id
                        == manifest.receipt.submission_id
                    && marker
                        .commit
                        .artifact_ids
                        .iter()
                        .any(|id| id == artifact_id)
            }
            Err(_) => false,
        };
        if !committed {
            return Ok(None);
        }
        let preferences = self.preferences_locked()?;
        let (available, corrupt) = self.payload_status(&manifest);
        Ok(Some(ArtifactRecord {
            manifest,
            preference: preferences
                .get(artifact_id)
                .cloned()
                .unwrap_or_default(),
            available,
            corrupt,
        }))
    }

    /// Read a committed snapshot after verifying its declared size and SHA-256.
    pub fn read_payload(&self, artifact_id: &str) -> io::Result<Vec<u8>> {
        validate_identifier(artifact_id)?;
        let _guard = self.lock();
        self.read_payload_locked(artifact_id)
    }

    fn read_payload_locked(&self, artifact_id: &str) -> io::Result<Vec<u8>> {
        let manifest = self.committed_manifest_locked(artifact_id)?;
        if !matches!(manifest.publication_state, PublicationState::Committed) {
            return Err(invalid("artifact deletion is pending"));
        }
        if manifest.receipt.size_bytes > MAX_ARTIFACT_BYTES {
            return Err(invalid("artifact manifest exceeds the read limit"));
        }
        let payload = self.payload_path(artifact_id);
        reject_link(&payload)?;
        let file = File::open(payload)?;
        let mut bytes = Vec::with_capacity(
            usize::try_from(manifest.receipt.size_bytes).unwrap_or(0),
        );
        file.take(MAX_ARTIFACT_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)?;
        let digest = sha256_hex(&bytes);
        if bytes.len() as u64 != manifest.receipt.size_bytes
            || digest != manifest.receipt.sha256
        {
            return Err(invalid(
                "artifact snapshot failed integrity verification",
            ));
        }
        Ok(bytes)
    }

    pub fn set_favorite(
        &self,
        artifact_id: &str,
        favorite: bool,
    ) -> io::Result<()> {
        validate_identifier(artifact_id)?;
        let _guard = self.lock();
        if !self.is_committed_locked(artifact_id)? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "artifact not found",
            ));
        }
        let mut entries = self.preferences_locked()?;
        entries.entry(artifact_id.to_owned()).or_default().favorite = favorite;
        self.save_preferences_locked(&entries)
    }

    pub fn trash(&self, artifact_id: &str) -> io::Result<()> {
        self.update_trashed_at(artifact_id, Some(now_ms()))
    }
    pub fn restore_from_trash(&self, artifact_id: &str) -> io::Result<()> {
        self.update_trashed_at(artifact_id, None)
    }

    fn update_trashed_at(
        &self,
        artifact_id: &str,
        trashed_at_ms: Option<u64>,
    ) -> io::Result<()> {
        validate_identifier(artifact_id)?;
        let _guard = self.lock();
        if !self.is_committed_locked(artifact_id)? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "artifact not found",
            ));
        }
        let mut entries = self.preferences_locked()?;
        entries
            .entry(artifact_id.to_owned())
            .or_default()
            .trashed_at_ms = trashed_at_ms;
        self.save_preferences_locked(&entries)
    }

    /// Permanently remove a currently trashed, committed payload only after
    /// atomically recording its tombstone. Re-checking trash state under the
    /// writer lock prevents a late confirmation from deleting a restored file.
    pub fn permanently_delete(&self, artifact_id: &str) -> io::Result<()> {
        validate_identifier(artifact_id)?;
        let _guard = self.lock();
        let path = self.item_dir(artifact_id).join("manifest.json");
        let mut manifest = read_manifest(&path)?;
        if manifest.receipt.artifact_id != artifact_id {
            return Err(invalid(
                "artifact manifest ID does not match its directory",
            ));
        }
        match manifest.publication_state {
            PublicationState::Deleted => return Ok(()),
            PublicationState::Prepared => {
                return Err(invalid(
                    "a prepared file cannot be permanently deleted through the library",
                ));
            }
            PublicationState::Committed => {
                if !self.committed_ids_locked()?.contains(artifact_id) {
                    return Err(invalid(
                        "artifact has no committed delivery marker",
                    ));
                }
                let preferences = self.preferences_locked()?;
                if !preferences.get(artifact_id).is_some_and(|preference| {
                    preference.trashed_at_ms.is_some()
                }) {
                    return Err(invalid(
                        "artifact must still be in the trash before permanent deletion",
                    ));
                }
                manifest.publication_state = PublicationState::DeletePending;
                self.write_metadata_locked(&path, &manifest)?;
            }
            // This durable tombstone proves a prior confirmed deletion crossed
            // its barrier. Retry occupied-file cleanup without requiring the
            // preference file to be readable again.
            PublicationState::DeletePending => {}
        }
        match fs::remove_file(self.payload_path(artifact_id)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        manifest.publication_state = PublicationState::Deleted;
        self.write_metadata_locked(&path, &manifest)
    }

    /// Copy a verified immutable snapshot to a user-selected path after the
    /// native save dialog has confirmed replacement of any existing file.
    pub fn copy_to(
        &self,
        artifact_id: &str,
        destination: &Path,
    ) -> io::Result<()> {
        validate_identifier(artifact_id)?;
        let _guard = self.lock();
        let parent = destination
            .parent()
            .ok_or_else(|| invalid("destination has no parent directory"))?;
        let canonical_parent = fs::canonicalize(parent)?;
        if canonical_parent.starts_with(&self.inner.root) {
            return Err(invalid("cannot save over managed artifact storage"));
        }
        if destination.exists() {
            reject_link(destination)?;
            let canonical_destination = fs::canonicalize(destination)?;
            if canonical_destination.starts_with(&self.inner.root) {
                return Err(invalid(
                    "cannot save over managed artifact storage",
                ));
            }
        }
        let bytes = self.read_payload_locked(artifact_id)?;
        atomic_write(destination, &bytes)
    }

    /// Create an extension-preserving private copy for a system file opener.
    /// Callers own process lifecycle and remove it only when safe to do so.
    pub fn create_view_copy(&self, artifact_id: &str) -> io::Result<ViewCopy> {
        validate_identifier(artifact_id)?;
        let _guard = self.lock();
        let record = self
            .list_locked(true)?
            .into_iter()
            .find(|record| record.manifest.receipt.artifact_id == artifact_id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "artifact is not committed",
                )
            })?;
        if !can_open_with_default_app(&record.manifest.receipt.display_name) {
            return Err(invalid(
                "this file type must be saved explicitly before opening",
            ));
        }
        let bytes = self.read_payload_locked(artifact_id)?;
        let scan = self.scan_locked()?;
        if scan.payload_bytes.saturating_add(bytes.len() as u64)
            > MAX_STORE_BYTES
        {
            return Err(invalid("artifact library storage limit reached"));
        }
        let digest = sha256_hex(&bytes);
        let created_at_ms = self
            .max_view_copy_timestamp_locked()
            .map_or_else(now_ms, |previous| {
                now_ms().max(previous.saturating_add(1))
            });
        let request_id = random_id()?;
        let dir = self.inner.root.join("views").join(&request_id);
        fs::create_dir(&dir)?;
        let extension = Path::new(&record.manifest.receipt.display_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| {
                extension.len() <= 16
                    && extension.chars().all(|c| c.is_ascii_alphanumeric())
            })
            .unwrap_or("bin");
        let file_name = format!("artifact.{extension}");
        let path = dir.join(&file_name);
        if let Err(error) = write_new_synced(&path, &bytes) {
            let _ = fs::remove_dir_all(&dir);
            return Err(error);
        }
        let meta_dir = self.inner.root.join("views-meta");
        if let Err(error) =
            fs::create_dir_all(&meta_dir).and_then(|()| reject_link(&meta_dir))
        {
            let _ = fs::remove_dir_all(&dir);
            return Err(error);
        }
        let meta = ViewCopyMeta {
            request_id: request_id.clone(),
            artifact_id: artifact_id.to_owned(),
            file_name,
            sha256: digest,
            created_at_ms,
        };
        let meta_bytes = match serde_json::to_vec(&meta) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = fs::remove_dir_all(&dir);
                return Err(io::Error::other(error));
            }
        };
        if let Err(error) =
            atomic_write(&self.view_meta_path(&request_id), &meta_bytes)
        {
            let _ = fs::remove_dir_all(&dir);
            return Err(error);
        }
        if let Err(error) = self.cleanup_stale_view_copies_locked(created_at_ms)
        {
            tracing::warn!(%error, "stale view copy cleanup deferred");
        }
        Ok(ViewCopy { request_id, path })
    }

    pub fn remove_view_copy(&self, request_id: &str) -> io::Result<()> {
        validate_identifier(request_id)?;
        let _guard = self.lock();
        let path = self.inner.root.join("views").join(request_id);
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        remove_file_if_exists(&self.view_meta_path(request_id))
    }

    /// Garbage-collect unmodified view copies outside the newest
    /// `MAX_RETAINED_VIEW_COPIES` within `VIEW_COPY_RETENTION_MS`, along with
    /// orphaned sidecar metadata files. Modified, legacy, or locked view
    /// copies are preserved untouched. Returns the number of evicted view
    /// directories.
    pub fn cleanup_stale_view_copies(&self) -> io::Result<usize> {
        let _guard = self.lock();
        self.cleanup_stale_view_copies_locked(now_ms())
    }

    #[cfg(test)]
    fn cleanup_stale_view_copies_at(&self, now_ms: u64) -> io::Result<usize> {
        let _guard = self.lock();
        self.cleanup_stale_view_copies_locked(now_ms)
    }

    fn cleanup_stale_view_copies_locked(
        &self,
        now_ms: u64,
    ) -> io::Result<usize> {
        let views_dir = self.inner.root.join("views");
        let meta_dir = self.inner.root.join("views-meta");
        reject_link(&views_dir)?;
        if meta_dir.exists() {
            reject_link(&meta_dir)?;
        }

        let mut managed = Vec::new();
        for entry in fs::read_dir(&views_dir)? {
            let entry = entry?;
            let dir = entry.path();
            let Ok(dir_meta) = fs::symlink_metadata(&dir) else {
                continue;
            };
            if dir_meta.file_type().is_symlink()
                || is_windows_reparse_point(&dir_meta)
                || !dir_meta.is_dir()
            {
                continue;
            }
            let Some(dir_name) = entry.file_name().to_str().map(str::to_owned)
            else {
                continue;
            };
            if validate_identifier(&dir_name).is_err() {
                continue;
            }
            let meta_path = self.view_meta_path(&dir_name);
            let Ok(meta) = read_limited_json::<ViewCopyMeta>(
                &meta_path,
                MAX_MANIFEST_BYTES,
            ) else {
                continue;
            };
            if meta.request_id != dir_name
                || validate_identifier(&meta.request_id).is_err()
                || validate_identifier(&meta.artifact_id).is_err()
                || !is_single_file_name(&meta.file_name)
            {
                continue;
            }
            let Some(file_path) =
                inspect_managed_view_dir(&dir, &meta.file_name)
            else {
                continue;
            };
            managed.push(ManagedViewCopy {
                meta,
                dir,
                file_path,
                meta_path,
            });
        }

        managed.sort_by(|a, b| {
            b.meta
                .created_at_ms
                .cmp(&a.meta.created_at_ms)
                .then_with(|| b.meta.request_id.cmp(&a.meta.request_id))
        });

        let mut removed = 0_usize;
        for (rank, candidate) in managed.into_iter().enumerate() {
            let within_count = rank < MAX_RETAINED_VIEW_COPIES;
            let within_age = now_ms
                .saturating_sub(candidate.meta.created_at_ms)
                < VIEW_COPY_RETENTION_MS;
            if within_count && within_age {
                continue;
            }
            let Ok(file_meta) = fs::symlink_metadata(&candidate.file_path)
            else {
                continue;
            };
            if file_meta.file_type().is_symlink()
                || is_windows_reparse_point(&file_meta)
                || !file_meta.is_file()
            {
                continue;
            }
            let mut file = match File::open(&candidate.file_path) {
                Ok(file) => file,
                Err(error) => {
                    tracing::warn!(
                        path = %candidate.file_path.display(),
                        %error,
                        "stale view copy could not be opened for verification"
                    );
                    continue;
                }
            };
            let snapshot = match snapshot_file(&mut file) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    tracing::warn!(
                        path = %candidate.file_path.display(),
                        %error,
                        "stale view copy could not be hashed; preserving"
                    );
                    continue;
                }
            };
            drop(file);
            if snapshot.digest != candidate.meta.sha256 {
                continue;
            }
            if let Err(error) = reject_link(&candidate.dir)
                .and_then(|()| fs::remove_dir_all(&candidate.dir))
            {
                tracing::warn!(
                    path = %candidate.dir.display(),
                    %error,
                    "stale view copy could not be removed; retaining"
                );
                continue;
            }
            if let Err(error) = remove_file_if_exists(&candidate.meta_path) {
                tracing::warn!(
                    path = %candidate.meta_path.display(),
                    %error,
                    "evicted view copy metadata could not be removed"
                );
            }
            removed = removed.saturating_add(1);
        }

        if meta_dir.exists() {
            for entry in fs::read_dir(&meta_dir)? {
                let entry = entry?;
                let path = entry.path();
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if metadata.file_type().is_symlink()
                    || is_windows_reparse_point(&metadata)
                    || !metadata.is_file()
                {
                    continue;
                }
                let Some(file_name) =
                    entry.file_name().to_str().map(str::to_owned)
                else {
                    continue;
                };
                let Some(request_id) = file_name.strip_suffix(".json") else {
                    continue;
                };
                if validate_identifier(request_id).is_err() {
                    continue;
                }
                if matches!(
                    fs::symlink_metadata(views_dir.join(request_id)),
                    Err(error) if error.kind() == io::ErrorKind::NotFound
                ) && let Err(error) = remove_file_if_exists(&path)
                {
                    tracing::warn!(
                        path = %path.display(),
                        %error,
                        "orphaned view copy metadata could not be removed"
                    );
                }
            }
        }

        Ok(removed)
    }

    fn max_view_copy_timestamp_locked(&self) -> Option<u64> {
        let entries = fs::read_dir(self.inner.root.join("views-meta")).ok()?;
        entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|ext| ext.to_str())
                    == Some("json")
            })
            .filter_map(|entry| {
                read_limited_json::<ViewCopyMeta>(
                    &entry.path(),
                    MAX_MANIFEST_BYTES,
                )
                .ok()
                .map(|meta| meta.created_at_ms)
            })
            .max()
    }

    fn commit_delivery(&self, commit: &DeliveryCommit) -> io::Result<()> {
        validate_identifier(&commit.session_id)?;
        validate_identifier(&commit.submission_id)?;
        if !commit.completed_successfully {
            return Err(invalid("cannot commit an unsuccessful delivery"));
        }
        if !origin_may_own_artifacts(&commit.origin) {
            return Err(invalid(
                "only explicit Home or Project sessions may own artifacts",
            ));
        }
        let expected =
            commit.artifact_ids.iter().cloned().collect::<BTreeSet<_>>();
        if expected.len() != commit.artifact_ids.len()
            || expected.iter().any(|id| validate_identifier(id).is_err())
        {
            return Err(invalid(
                "delivery commit contains invalid or duplicate IDs",
            ));
        }
        if expected.is_empty() {
            return Ok(());
        }

        let _guard = self.lock();
        let marker_path =
            self.delivery_path(&commit.session_id, &commit.submission_id);
        if marker_path.exists() {
            let marker = self.read_delivery_marker(&marker_path)?;
            if marker.commit.session_id == commit.session_id
                && marker.commit.submission_id == commit.submission_id
                && marker.commit.artifact_ids == commit.artifact_ids
                && marker.commit.completed_successfully
            {
                self.refresh_delivery_title_locked(commit)?;
                if marker.commit.origin_title_snapshot
                    != commit.origin_title_snapshot
                {
                    self.write_metadata_locked(
                        &marker_path,
                        &DeliveryMarker {
                            schema_version: 1,
                            commit: commit.clone(),
                        },
                    )?;
                }
                if let Err(error) =
                    self.prune_superseded_conversation_imports_locked()
                {
                    tracing::warn!(%error, "superseded conversation artifacts retained for retry");
                }
                return Ok(());
            }
            return Err(invalid(
                "delivery marker already exists with a different receipt set",
            ));
        }
        let scan = self.scan_locked()?;
        let mut candidates = BTreeMap::new();
        for manifest in scan.manifests.iter().filter(|manifest| {
            manifest.receipt.session_id == commit.session_id
                && manifest.receipt.submission_id == commit.submission_id
        }) {
            candidates
                .insert(manifest.receipt.artifact_id.clone(), manifest.clone());
        }
        if candidates.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(invalid(
                "commit receipt set does not match prepared artifacts",
            ));
        }
        if candidates.values().any(|manifest| {
            manifest.origin != commit.origin
                || !origin_may_own_artifacts(&manifest.origin)
                || matches!(
                    manifest.publication_state,
                    PublicationState::Deleted | PublicationState::DeletePending
                )
        }) {
            return Err(invalid(
                "delivery contains a mismatched or deleted artifact",
            ));
        }
        if candidates.len() > MAX_DELIVERY_FILES
            || candidates
                .values()
                .map(|manifest| manifest.receipt.size_bytes)
                .sum::<u64>()
                > MAX_DELIVERY_BYTES
        {
            return Err(invalid("delivery exceeds the per-turn byte budget"));
        }

        for artifact_id in &commit.artifact_ids {
            let staged = self.staging_dir(
                &commit.session_id,
                &commit.submission_id,
                artifact_id,
            );
            let item = self.item_dir(artifact_id);
            if item.exists() {
                let manifest_path = if item.join("manifest.json").exists() {
                    item.join("manifest.json")
                } else {
                    item.join("prepared.json")
                };
                let mut manifest = read_manifest(&manifest_path)?;
                if manifest.receipt.artifact_id != *artifact_id
                    || manifest.receipt.session_id != commit.session_id
                    || manifest.receipt.submission_id != commit.submission_id
                {
                    return Err(invalid(
                        "existing item does not match the delivery commit",
                    ));
                }
                if matches!(
                    manifest.publication_state,
                    PublicationState::Deleted | PublicationState::DeletePending
                ) {
                    return Err(invalid(
                        "permanently removed artifact cannot be restored",
                    ));
                }
                Self::verify_payload(
                    &self.payload_path(artifact_id),
                    &manifest.receipt,
                )?;
                manifest.publication_state = PublicationState::Committed;
                manifest
                    .origin_title_snapshot
                    .clone_from(&commit.origin_title_snapshot);
                manifest
                    .receipt
                    .origin_title_snapshot
                    .clone_from(&commit.origin_title_snapshot);
                self.write_metadata_locked(
                    &item.join("manifest.json"),
                    &manifest,
                )?;
                continue;
            }
            let prepared = read_manifest(&staged.join("prepared.json"))?;
            if prepared.receipt.artifact_id != *artifact_id
                || prepared.receipt.session_id != commit.session_id
                || prepared.receipt.submission_id != commit.submission_id
            {
                return Err(invalid(
                    "prepared artifact does not match the delivery commit",
                ));
            }
            Self::verify_payload(&staged.join("payload"), &prepared.receipt)?;
            fs::create_dir_all(item.parent().expect("item path has parent"))?;
            fs::rename(&staged, &item)?;
            let manifest_path = item.join("prepared.json");
            let mut manifest = read_manifest(&manifest_path)?;
            manifest.publication_state = PublicationState::Committed;
            manifest
                .origin_title_snapshot
                .clone_from(&commit.origin_title_snapshot);
            manifest
                .receipt
                .origin_title_snapshot
                .clone_from(&commit.origin_title_snapshot);
            fs::rename(&manifest_path, item.join("manifest.json"))?;
            self.write_metadata_locked(&item.join("manifest.json"), &manifest)?;
        }
        let marker = DeliveryMarker {
            schema_version: 1,
            commit: commit.clone(),
        };
        self.write_metadata_locked(&marker_path, &marker)?;
        if let Err(error) = self.prune_superseded_conversation_imports_locked()
        {
            tracing::warn!(%error, "superseded conversation artifacts retained for retry");
        }
        Ok(())
    }

    fn refresh_delivery_title_locked(
        &self,
        commit: &DeliveryCommit,
    ) -> io::Result<()> {
        for artifact_id in &commit.artifact_ids {
            let manifest_path =
                self.item_dir(artifact_id).join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }
            let mut manifest = read_manifest(&manifest_path)?;
            if manifest.receipt.artifact_id != *artifact_id
                || manifest.receipt.session_id != commit.session_id
                || manifest.receipt.submission_id != commit.submission_id
            {
                return Err(invalid(
                    "artifact manifest does not match the delivery title update",
                ));
            }
            if matches!(manifest.publication_state, PublicationState::Deleted)
                || (manifest.origin_title_snapshot
                    == commit.origin_title_snapshot
                    && manifest.receipt.origin_title_snapshot
                        == commit.origin_title_snapshot)
            {
                continue;
            }
            manifest
                .origin_title_snapshot
                .clone_from(&commit.origin_title_snapshot);
            manifest
                .receipt
                .origin_title_snapshot
                .clone_from(&commit.origin_title_snapshot);
            self.write_metadata_locked(&manifest_path, &manifest)?;
        }
        Ok(())
    }

    fn committed_manifest_locked(
        &self,
        artifact_id: &str,
    ) -> io::Result<ArtifactManifest> {
        let manifest =
            read_manifest(&self.item_dir(artifact_id).join("manifest.json"))?;
        if manifest.receipt.artifact_id != artifact_id
            || !matches!(
                manifest.publication_state,
                PublicationState::Committed
            )
            || !self.committed_ids_locked()?.contains(artifact_id)
        {
            return Err(invalid(
                "artifact is not available as a committed file",
            ));
        }
        Ok(manifest)
    }

    fn verify_payload(
        path: &Path,
        receipt: &ArtifactReceipt,
    ) -> io::Result<()> {
        let mut file = File::open(path)?;
        let snapshot = snapshot_file(&mut file)?;
        if snapshot.size != receipt.size_bytes
            || snapshot.digest != receipt.sha256
        {
            return Err(invalid(
                "prepared snapshot failed integrity verification",
            ));
        }
        Ok(())
    }

    fn is_committed_locked(&self, artifact_id: &str) -> io::Result<bool> {
        let manifest =
            read_manifest(&self.item_dir(artifact_id).join("manifest.json"))?;
        Ok(
            matches!(manifest.publication_state, PublicationState::Committed)
                && self.committed_ids_locked()?.contains(artifact_id),
        )
    }

    fn read_delivery_marker(&self, path: &Path) -> io::Result<DeliveryMarker> {
        let marker: DeliveryMarker =
            read_limited_json(path, MAX_MANIFEST_BYTES)?;
        if marker.schema_version != 1
            || !marker.commit.completed_successfully
            || !origin_may_own_artifacts(&marker.commit.origin)
            || marker.commit.artifact_ids.is_empty()
            || marker.commit.artifact_ids.len() > MAX_DELIVERY_FILES
        {
            return Err(invalid("unsupported or invalid delivery marker"));
        }
        validate_identifier(&marker.commit.session_id)?;
        validate_identifier(&marker.commit.submission_id)?;
        if path
            != self.delivery_path(
                &marker.commit.session_id,
                &marker.commit.submission_id,
            )
        {
            return Err(invalid(
                "delivery marker identity does not match its filename",
            ));
        }
        let mut ids = BTreeSet::new();
        for artifact_id in &marker.commit.artifact_ids {
            validate_identifier(artifact_id)?;
            if !ids.insert(artifact_id) {
                return Err(invalid(
                    "delivery marker contains duplicate artifact IDs",
                ));
            }
        }
        Ok(marker)
    }

    fn committed_ids_locked(&self) -> io::Result<BTreeSet<String>> {
        let dir = self.inner.root.join("deliveries");
        let mut ids = BTreeSet::new();
        let mut count = 0_usize;
        let mut metadata_bytes = 0_u64;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            count += 1;
            if count > MAX_SCANNED_ENTRIES {
                return Err(invalid("delivery marker scan limit exceeded"));
            }
            if entry.file_type()?.is_symlink() || !entry.file_type()?.is_file()
            {
                continue;
            }
            if entry.path().extension().and_then(|value| value.to_str())
                != Some("json")
            {
                continue;
            }
            metadata_bytes =
                metadata_bytes.saturating_add(entry.metadata()?.len());
            if metadata_bytes > MAX_METADATA_BYTES {
                return Err(invalid("delivery metadata budget exceeded"));
            }
            let marker = match self.read_delivery_marker(&entry.path()) {
                Ok(marker) => marker,
                Err(error) => {
                    tracing::warn!(path = %entry.path().display(), %error, "artifact marker is unreadable; retained for diagnosis");
                    continue;
                }
            };
            ids.extend(marker.commit.artifact_ids);
        }
        Ok(ids)
    }

    fn scan_locked(&self) -> io::Result<Scan> {
        self.scan_with_diagnostics_locked(true)
    }

    fn scan_with_diagnostics_locked(&self, strict: bool) -> io::Result<Scan> {
        let mut scan = Scan::default();
        let mut directories = vec![self.inner.root.clone()];
        let mut manifests = BTreeMap::<PathBuf, PathBuf>::new();
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                scan.entry_count = scan.entry_count.saturating_add(1);
                if scan.entry_count > MAX_SCANNED_ENTRIES {
                    return Err(invalid("artifact scan limit exceeded"));
                }
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.file_type().is_symlink()
                    || is_windows_reparse_point(&metadata)
                {
                    let diagnostic = format!(
                        "managed artifact path is a link: {}",
                        path.display()
                    );
                    if strict {
                        return Err(invalid(diagnostic));
                    }
                    scan.diagnostics.push(diagnostic);
                    continue;
                }
                if metadata.is_dir() {
                    directories.push(path);
                    continue;
                }
                if !metadata.is_file() {
                    return Err(invalid(
                        "managed artifact path is not a regular file",
                    ));
                }
                let is_view = path.starts_with(self.inner.root.join("views"));
                let is_payload = entry.file_name() == "payload";
                if is_view || is_payload {
                    scan.payload_bytes =
                        scan.payload_bytes.saturating_add(metadata.len());
                    if is_view {
                        scan.view_bytes =
                            scan.view_bytes.saturating_add(metadata.len());
                    } else if path.starts_with(self.inner.root.join("staging"))
                    {
                        scan.staging_bytes =
                            scan.staging_bytes.saturating_add(metadata.len());
                    } else {
                        scan.stored_bytes =
                            scan.stored_bytes.saturating_add(metadata.len());
                    }
                    if is_payload {
                        scan.payload_files =
                            scan.payload_files.saturating_add(1);
                    }
                } else {
                    // Includes abandoned temporary writes, preferences, source
                    // deletion records, markers and unknown diagnostics. A
                    // crash cannot turn unmanaged bytes into free quota.
                    scan.metadata_bytes =
                        scan.metadata_bytes.saturating_add(metadata.len());
                }
                if path.starts_with(self.inner.root.join("deliveries"))
                    && path.extension().and_then(|value| value.to_str())
                        == Some("json")
                    && let Err(error) = self.read_delivery_marker(&path)
                {
                    if strict {
                        return Err(error);
                    }
                    scan.diagnostics
                        .push(format!("{}: {error}", path.display()));
                }
                if entry.file_name() == "manifest.json"
                    || entry.file_name() == "prepared.json"
                {
                    let parent = path
                        .parent()
                        .expect("managed file has parent")
                        .to_path_buf();
                    match manifests.get(&parent) {
                        Some(_) if entry.file_name() == "prepared.json" => {}
                        _ => {
                            manifests.insert(parent, path);
                        }
                    }
                }
            }
        }
        if scan.metadata_bytes > MAX_METADATA_BYTES {
            return Err(invalid("artifact metadata budget exceeded"));
        }
        if scan.payload_files > MAX_STORED_ITEMS {
            return Err(invalid(
                "artifact payload record count limit exceeded",
            ));
        }
        for (directory, path) in manifests {
            let result = read_manifest(&path).and_then(|manifest| {
                if manifest.receipt.artifact_id
                    != directory
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                {
                    return Err(invalid(
                        "artifact manifest ID does not match its directory",
                    ));
                }
                validate_identifier(&manifest.receipt.artifact_id)?;
                Ok(manifest)
            });
            match result {
                Ok(manifest) => scan.manifests.push(manifest),
                Err(error) if strict => return Err(error),
                Err(error) => scan
                    .diagnostics
                    .push(format!("{}: {error}", path.display())),
            }
        }
        Ok(scan)
    }
    fn preferences_locked(
        &self,
    ) -> io::Result<BTreeMap<String, ArtifactPreference>> {
        let path = self.inner.root.join("preferences.json");
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let file =
            read_limited_json::<PreferencesFile>(&path, MAX_METADATA_BYTES)?;
        if file.schema_version > 1 {
            return Err(invalid(
                "unsupported future artifact preferences version",
            ));
        }
        Ok(file.entries)
    }

    fn save_preferences_locked(
        &self,
        entries: &BTreeMap<String, ArtifactPreference>,
    ) -> io::Result<()> {
        let file = PreferencesFile {
            schema_version: 1,
            entries: entries.clone(),
        };
        let bytes = serde_json::to_vec(&file).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_METADATA_BYTES {
            return Err(invalid("artifact preferences exceed metadata budget"));
        }
        self.reserve_metadata_locked(&bytes, 1)?;
        atomic_write(&self.inner.root.join("preferences.json"), &bytes)
    }

    fn reserve_metadata_locked(
        &self,
        bytes: &[u8],
        entries: usize,
    ) -> io::Result<()> {
        let scan = self.scan_locked()?;
        if scan.metadata_bytes.saturating_add(bytes.len() as u64)
            > MAX_METADATA_BYTES
            || scan.entry_count.saturating_add(entries) > MAX_SCANNED_ENTRIES
        {
            return Err(invalid(
                "artifact metadata or scan-entry limit reached",
            ));
        }
        Ok(())
    }

    fn write_metadata_locked(
        &self,
        path: &Path,
        value: &impl Serialize,
    ) -> io::Result<()> {
        let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(invalid(
                "managed metadata record exceeds its size limit",
            ));
        }
        self.reserve_metadata_locked(&bytes, 2)?;
        atomic_write(path, &bytes)
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.inner
            .serial
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn staging_dir(
        &self,
        session: &str,
        submission: &str,
        artifact: &str,
    ) -> PathBuf {
        self.inner
            .root
            .join("staging")
            .join(session)
            .join(submission)
            .join(artifact)
    }
    fn item_dir(&self, artifact: &str) -> PathBuf {
        self.inner.root.join("items").join(artifact)
    }
    fn payload_path(&self, artifact: &str) -> PathBuf {
        self.item_dir(artifact).join("payload")
    }
    fn delivery_path(&self, session: &str, submission: &str) -> PathBuf {
        let key = format!("{session}\0{submission}");
        self.inner
            .root
            .join("deliveries")
            .join(format!("{}.json", hex(&Sha256::digest(key.as_bytes()))))
    }
    fn session_deletion_path(&self, session_id: &str) -> PathBuf {
        self.inner
            .root
            .join("session-deletions")
            .join(format!("{session_id}.json"))
    }
    fn source_deletion_path(&self, session_id: &str) -> PathBuf {
        self.inner
            .root
            .join("sources-deleted")
            .join(format!("{session_id}.json"))
    }
    fn view_meta_path(&self, request_id: &str) -> PathBuf {
        self.inner
            .root
            .join("views-meta")
            .join(format!("{request_id}.json"))
    }
}

struct ManagedViewCopy {
    meta: ViewCopyMeta,
    dir: PathBuf,
    file_path: PathBuf,
    meta_path: PathBuf,
}

#[derive(Default)]
struct Scan {
    manifests: Vec<ArtifactManifest>,
    payload_bytes: u64,
    payload_files: usize,
    metadata_bytes: u64,
    entry_count: usize,
    diagnostics: Vec<String>,
    stored_bytes: u64,
    staging_bytes: u64,
    view_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SourceDeletion {
    schema_version: u32,
    session_id: String,
    deleted_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDeletionState {
    Prepared,
    Committed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionDeletionPlan {
    schema_version: u32,
    pub session_id: String,
    pub state: SessionDeletionState,
    artifact_ids: Vec<String>,
    submission_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_session_ids: Vec<String>,
    created_at_ms: u64,
}

#[async_trait::async_trait]
impl DeliveryCommitter for ArtifactStore {
    async fn commit(&self, commit: &DeliveryCommit) -> Result<(), String> {
        let store = self.clone();
        let commit = commit.clone();
        tokio::task::spawn_blocking(move || store.commit_delivery(&commit))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
    }
}

fn snapshot_file(file: &mut File) -> io::Result<Snapshot> {
    file.rewind()?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut prefix = Vec::with_capacity(16);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(read as u64);
        if size > MAX_ARTIFACT_BYTES {
            return Err(invalid("artifact exceeds the 128 MiB per-file limit"));
        }
        if prefix.len() < 16 {
            let remaining = 16 - prefix.len();
            prefix.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Snapshot {
        digest: hex(&hasher.finalize()),
        size,
        prefix,
    })
}

fn copy_and_hash(
    source: &mut File,
    destination: &mut File,
) -> io::Result<Snapshot> {
    source.rewind()?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut prefix = Vec::with_capacity(16);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(read as u64);
        if size > MAX_ARTIFACT_BYTES {
            return Err(invalid("artifact exceeds the 128 MiB per-file limit"));
        }
        if prefix.len() < 16 {
            let remaining = 16 - prefix.len();
            prefix.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        hasher.update(&buffer[..read]);
        destination.write_all(&buffer[..read])?;
    }
    Ok(Snapshot {
        digest: hex(&hasher.finalize()),
        size,
        prefix,
    })
}

const fn origin_may_own_artifacts(origin: &SessionOrigin) -> bool {
    matches!(origin, SessionOrigin::Home | SessionOrigin::Project { .. })
}

fn validate_relative_path(raw: &str) -> io::Result<PathBuf> {
    if raw.trim().is_empty() || raw.chars().any(char::is_control) {
        return Err(invalid("path must be a non-empty relative file path"));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(invalid("absolute paths are not accepted"));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(invalid("path traversal components are not accepted"));
        }
    }
    Ok(path.to_path_buf())
}

fn reject_link(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || is_windows_reparse_point(&metadata)
    {
        return Err(invalid(
            "symbolic links and reparse points are not accepted",
        ));
    }
    Ok(())
}

fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    flowy_agent_core::tools::is_windows_reparse_point(metadata)
}

fn validate_identifier(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|char| {
            char.is_ascii_alphanumeric() || matches!(char, '-' | '_')
        })
    {
        return Err(invalid("invalid host identifier"));
    }
    Ok(())
}

fn validate_delivery_key(value: &str) -> io::Result<()> {
    let length = value.chars().count();
    if !(1..=128).contains(&length) || value.chars().any(char::is_control) {
        return Err(invalid(
            "delivery_key must contain 1 to 128 printable characters",
        ));
    }
    Ok(())
}

fn safe_display_name(
    provided: Option<&str>,
    source_name: &str,
) -> io::Result<String> {
    let source_extension = Path::new(source_name)
        .extension()
        .and_then(|value| value.to_str());
    let candidate = provided.unwrap_or(source_name);
    if candidate.chars().count() > 255 {
        return Err(invalid("display_name exceeds 255 Unicode scalar values"));
    }
    let mut safe = candidate
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .to_owned();
    if safe.is_empty() || safe == "." || safe == ".." {
        return Err(invalid("display_name is empty or invalid"));
    }
    let provided_extension = Path::new(&safe)
        .extension()
        .and_then(|value| value.to_str());
    if let Some(extension) = source_extension
        && provided_extension
            .is_none_or(|given| !given.eq_ignore_ascii_case(extension))
    {
        safe.push('.');
        safe.push_str(extension);
    }
    if safe.chars().count() > 255 {
        return Err(invalid("display_name exceeds 255 Unicode scalar values"));
    }
    Ok(safe)
}

fn media_type(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let media_type = match extension.as_str() {
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "pptx" => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        "docx" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        "xlsx" => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        }
        "zip" => "application/zip",
        _ => return None,
    };
    Some(media_type.to_owned())
}

fn validate_known_header(path: &str, prefix: &[u8]) -> io::Result<()> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let valid = match extension.as_str() {
        "pdf" => prefix.starts_with(b"%PDF-"),
        "png" => prefix.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => prefix.starts_with(&[0xff, 0xd8, 0xff]),
        "webp" => {
            prefix.len() >= 12
                && &prefix[0..4] == b"RIFF"
                && &prefix[8..12] == b"WEBP"
        }
        "pptx" | "docx" | "xlsx" | "zip" => prefix.starts_with(b"PK\x03\x04"),
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("file content does not match its extension"))
    }
}

fn read_session_deletion_plan(
    path: &Path,
    expected_session_id: &str,
) -> io::Result<SessionDeletionPlan> {
    let plan =
        read_limited_json::<SessionDeletionPlan>(path, MAX_MANIFEST_BYTES)?;
    if plan.schema_version != 1 || plan.session_id != expected_session_id {
        return Err(invalid("unsupported session deletion marker"));
    }
    Ok(plan)
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn read_manifest(path: &Path) -> io::Result<ArtifactManifest> {
    let manifest: ArtifactManifest =
        read_limited_json(path, MAX_MANIFEST_BYTES)?;
    if manifest.schema_version != 1 {
        return Err(invalid("unsupported artifact manifest version"));
    }
    Ok(manifest)
}

fn read_limited_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    limit: u64,
) -> io::Result<T> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || is_windows_reparse_point(&metadata)
        || metadata.len() > limit
    {
        return Err(invalid("managed metadata file is invalid or too large"));
    }
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("managed metadata file exceeds its read limit"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file =
        OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| invalid("metadata path has no parent"))?;
    fs::create_dir_all(parent)?;
    let (temporary, mut output) = loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary =
            parent.join(format!(".artifact-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(output) => break (temporary, output),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        output.write_all(bytes)?;
        output.sync_all()?;
        drop(output);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

fn random_id() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(hex(&bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn is_single_file_name(name: &str) -> bool {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(char::is_control)
        || name.contains(['/', '\\'])
    {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
}

fn inspect_managed_view_dir(
    dir: &Path,
    expected_file_name: &str,
) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut matched: Option<PathBuf> = None;
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink()
            || is_windows_reparse_point(&metadata)
            || !metadata.is_file()
        {
            return None;
        }
        if entry.file_name().to_str() != Some(expected_file_name)
            || matched.is_some()
        {
            return None;
        }
        matched = Some(path);
    }
    matched
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn now_ms() -> u64 {
    crate::handle::now_ms()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[must_use]
pub fn can_open_with_default_app(file_name: &str) -> bool {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "pdf"
            | "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "pptx"
            | "docx"
            | "xlsx"
            | "csv"
            | "tsv"
            | "txt"
            | "md"
            | "json"
            | "xml"
            | "rtf"
            | "odt"
            | "ods"
            | "odp"
    )
}

#[cfg(test)]
#[path = "artifact_store_tests.rs"]
mod tests;
