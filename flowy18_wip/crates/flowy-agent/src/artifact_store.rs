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
use std::time::{SystemTime, UNIX_EPOCH};

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
        for name in ["staging", "items", "deliveries", "views"] {
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

    pub(crate) fn record_source_deleted(
        &self,
        session_id: &str,
    ) -> io::Result<()> {
        validate_identifier(session_id)?;
        let _guard = self.lock();
        let path = self
            .inner
            .root
            .join("sources-deleted")
            .join(format!("{session_id}.json"));
        if path.exists() {
            let existing =
                read_limited_json::<SourceDeletion>(&path, MAX_MANIFEST_BYTES)?;
            if existing.schema_version != 1 || existing.session_id != session_id
            {
                return Err(invalid("unsupported source deletion marker"));
            }
            return Ok(());
        }
        self.write_metadata_locked(
            &path,
            &SourceDeletion {
                schema_version: 1,
                session_id: session_id.to_owned(),
                deleted_at_ms: now_ms(),
            },
        )
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
            created_at_ms: now_ms(),
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
        let preferences = self.preferences_locked()?;
        Ok(scan
            .manifests
            .into_iter()
            .filter(|manifest| {
                (matches!(
                    manifest.publication_state,
                    PublicationState::Committed
                        | PublicationState::DeletePending
                ) || (include_deleted
                    && matches!(
                        manifest.publication_state,
                        PublicationState::Deleted
                    )))
                    && committed.contains(&manifest.receipt.artifact_id)
            })
            .map(|manifest| {
                let artifact_id = manifest.receipt.artifact_id.clone();
                let available = matches!(
                    manifest.publication_state,
                    PublicationState::Committed
                ) && self.payload_path(&artifact_id).is_file();
                ArtifactRecord {
                    manifest,
                    preference: preferences
                        .get(&artifact_id)
                        .cloned()
                        .unwrap_or_default(),
                    available,
                }
            })
            .collect())
    }

    pub fn get(&self, artifact_id: &str) -> io::Result<Option<ArtifactRecord>> {
        validate_identifier(artifact_id)?;
        Ok(self
            .list_with_tombstones()?
            .into_iter()
            .find(|record| record.manifest.receipt.artifact_id == artifact_id))
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
        let digest = hex(&Sha256::digest(&bytes));
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
        let path = dir.join(format!("artifact.{extension}"));
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        Ok(ViewCopy { request_id, path })
    }

    pub fn remove_view_copy(&self, request_id: &str) -> io::Result<()> {
        validate_identifier(request_id)?;
        let _guard = self.lock();
        let path = self.inner.root.join("views").join(request_id);
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn commit_delivery(&self, commit: &DeliveryCommit) -> io::Result<()> {
        validate_identifier(&commit.session_id)?;
        validate_identifier(&commit.submission_id)?;
        if !commit.completed_successfully {
            return Err(invalid("cannot commit an unsuccessful delivery"));
        }
        if commit.origin != SessionOrigin::Home {
            return Err(invalid(
                "only explicitly marked Home sessions may publish artifacts",
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
            manifest.origin != SessionOrigin::Home
                || matches!(
                    manifest.publication_state,
                    PublicationState::Deleted | PublicationState::DeletePending
                )
        }) {
            return Err(invalid(
                "delivery contains a non-Home or deleted artifact",
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
            || marker.commit.origin != SessionOrigin::Home
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

#[async_trait::async_trait]
impl DeliveryCommitter for ArtifactStore {
    async fn commit(&self, commit: &DeliveryCommit) -> Result<(), String> {
        Self::commit_delivery(self, commit).map_err(|error| error.to_string())
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

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_windows_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
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
mod tests {
    use super::{
        ArtifactStore, PREPARED_RETENTION_MS, PrepareArtifact, PublicationState,
    };
    use flowy_agent_core::{DeliveryCommit, SessionOrigin};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("flowy-artifacts-{stamp}"));
            fs::create_dir_all(path.join("workspace/outputs")).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request(
        root: &PathBuf,
        relative_path: &str,
        key: &str,
    ) -> PrepareArtifact {
        PrepareArtifact {
            source_root: root.join("workspace"),
            relative_path: relative_path.into(),
            session_id: "session-1".into(),
            submission_id: "submission-1".into(),
            tool_call_id: format!("call-{key}"),
            delivery_key: key.into(),
            display_name: None,
            origin: SessionOrigin::Home,
        }
    }
    fn store(root: &PathBuf) -> ArtifactStore {
        fs::create_dir_all(root.join("workspace/outputs")).unwrap();
        ArtifactStore::open(root.join("artifacts")).unwrap()
    }
    fn commit(receipt: &flowy_agent_core::ArtifactReceipt) -> DeliveryCommit {
        DeliveryCommit {
            session_id: receipt.session_id.clone(),
            submission_id: receipt.submission_id.clone(),
            artifact_ids: vec![receipt.artifact_id.clone()],
            completed_successfully: true,
            origin: SessionOrigin::Home,
            origin_title_snapshot: "Home task".into(),
            committed_at_ms: 42,
        }
    }

    fn create_sparse_file(path: &std::path::Path, length: u64) {
        fs::File::create(path).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // SetEndOfFile alone does not promise a sparse NTFS allocation.
            // Mark the test-owned file before changing its logical length so
            // quota tests never reserve 5 GiB of real disk blocks.
            let output = std::process::Command::new("fsutil.exe")
                .args(["sparse", "setflag"])
                .arg(path)
                .creation_flags(0x0800_0000)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "could not mark test file sparse: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_len(length).unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn single_file_limit_accepts_exact_bounded_read_and_rejects_one_byte_over_before_copy()
     {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        let path = scratch.0.join("workspace/outputs/large.bin");
        create_sparse_file(&path, super::MAX_ARTIFACT_BYTES);
        // Exercise the actual bounded digest reader at exactly 128 MiB
        // without writing a 128 MiB snapshot into the managed store.
        let mut source = fs::File::open(&path).unwrap();
        let exact = super::snapshot_file(&mut source).unwrap();
        assert_eq!(exact.size, super::MAX_ARTIFACT_BYTES);
        assert_eq!(exact.prefix, vec![0; 16]);
        drop(source);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(super::MAX_ARTIFACT_BYTES + 1)
            .unwrap();
        let error = store
            .prepare(request(&scratch.0, "outputs/large.bin", "over-limit"))
            .unwrap_err();
        assert!(error.to_string().contains("128 MiB"));
        let usage = store.usage().unwrap();
        assert_eq!(usage.stored_items, 0);
        assert_eq!(usage.staging_bytes, 0);
        assert!(store.scan_locked().unwrap().manifests.is_empty());
    }

    #[test]
    fn ninth_file_is_rejected_without_damaging_the_first_eight() {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        let mut prepared = Vec::new();
        for index in 0..super::MAX_DELIVERY_FILES {
            let file = format!("report-{index}.csv");
            let bytes =
                format!("index,value\n{index},{}\n", index + 10).into_bytes();
            fs::write(scratch.0.join("workspace/outputs").join(&file), &bytes)
                .unwrap();
            let receipt = store
                .prepare(request(
                    &scratch.0,
                    &format!("outputs/{file}"),
                    &format!("slot-{index}"),
                ))
                .unwrap();
            prepared.push((receipt, bytes));
        }
        fs::write(
            scratch.0.join("workspace/outputs/ninth.csv"),
            b"index,value\n9,19\n",
        )
        .unwrap();
        let error = store
            .prepare(request(&scratch.0, "outputs/ninth.csv", "ninth"))
            .unwrap_err();
        assert!(error.to_string().contains("per-turn"));
        assert_eq!(
            store.usage().unwrap().stored_items,
            super::MAX_DELIVERY_FILES
        );
        assert_eq!(
            store.scan_locked().unwrap().manifests.len(),
            super::MAX_DELIVERY_FILES
        );
        assert!(
            store.list().unwrap().is_empty(),
            "prepared files are not delivered yet"
        );
        let mut proof = commit(&prepared[0].0);
        proof.artifact_ids = prepared
            .iter()
            .map(|(receipt, _)| receipt.artifact_id.clone())
            .collect();
        store.commit_delivery(&proof).unwrap();
        assert_eq!(store.list().unwrap().len(), super::MAX_DELIVERY_FILES);
        for (receipt, bytes) in prepared {
            assert_eq!(
                store.read_payload(&receipt.artifact_id).unwrap(),
                bytes
            );
        }
    }

    #[test]
    fn same_named_files_in_other_submissions_and_sessions_keep_independent_versions()
     {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        let source = scratch.0.join("workspace/outputs/report.csv");
        fs::write(&source, b"version,value\n1,old\n").unwrap();
        let first = store
            .prepare(request(&scratch.0, "outputs/report.csv", "report"))
            .unwrap();
        store.commit_delivery(&commit(&first)).unwrap();
        fs::write(&source, b"version,value\n2,new\n").unwrap();
        let mut next_turn = request(&scratch.0, "outputs/report.csv", "report");
        next_turn.submission_id = "submission-2".into();
        let second = store.prepare(next_turn).unwrap();
        store.commit_delivery(&commit(&second)).unwrap();
        // Identical bytes in a different source session are independently owned.
        fs::write(&source, b"version,value\n1,old\n").unwrap();
        let mut next_session =
            request(&scratch.0, "outputs/report.csv", "report");
        next_session.session_id = "session-2".into();
        let third = store.prepare(next_session).unwrap();
        store.commit_delivery(&commit(&third)).unwrap();
        assert_eq!(
            [&first.artifact_id, &second.artifact_id, &third.artifact_id]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3
        );
        assert_eq!(first.display_name, "report.csv");
        assert_eq!(second.display_name, first.display_name);
        assert_eq!(third.display_name, first.display_name);
        assert_eq!(first.sha256, third.sha256);
        assert_ne!(first.sha256, second.sha256);
        fs::remove_file(source).unwrap();
        assert_eq!(
            store.read_payload(&first.artifact_id).unwrap(),
            b"version,value\n1,old\n"
        );
        assert_eq!(
            store.read_payload(&second.artifact_id).unwrap(),
            b"version,value\n2,new\n"
        );
        assert_eq!(
            store.read_payload(&third.artifact_id).unwrap(),
            b"version,value\n1,old\n"
        );
        assert_eq!(store.list().unwrap().len(), 3);
    }

    #[test]
    fn sparse_raw_staging_counts_towards_total_quota_without_reading_its_bytes()
    {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        let raw = store.staging_dir(
            "quota-session",
            "quota-submission",
            "quota-reservation",
        );
        fs::create_dir_all(&raw).unwrap();
        let payload = raw.join("payload");
        create_sparse_file(&payload, super::MAX_STORE_BYTES);
        assert_eq!(
            store.usage().unwrap().staging_bytes,
            super::MAX_STORE_BYTES
        );
        fs::write(
            scratch.0.join("workspace/outputs/report.csv"),
            b"a,b\n1,2\n",
        )
        .unwrap();
        let error = store
            .prepare(request(
                &scratch.0,
                "outputs/report.csv",
                "quota-overflow",
            ))
            .unwrap_err();
        assert!(error.to_string().contains("storage limit"));
        let usage = store.usage().unwrap();
        assert_eq!(usage.staging_bytes, super::MAX_STORE_BYTES);
        assert_eq!(usage.stored_bytes, 0);
        assert_eq!(usage.stored_items, 1);
        assert_eq!(
            fs::metadata(payload).unwrap().len(),
            super::MAX_STORE_BYTES
        );
        assert!(store.scan_locked().unwrap().manifests.is_empty());
    }

    #[test]
    fn missing_empty_directory_and_mismatched_pdf_sources_never_prepare_a_delivery()
     {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::File::create(scratch.0.join("workspace/outputs/empty.csv"))
            .unwrap();
        fs::create_dir(scratch.0.join("workspace/outputs/folder")).unwrap();
        fs::write(
            scratch.0.join("workspace/outputs/pretend.pdf"),
            b"This is not PDF data.",
        )
        .unwrap();
        for (relative, key) in [
            ("outputs/missing.csv", "missing"),
            ("outputs/empty.csv", "empty"),
            ("outputs/folder", "directory"),
            ("outputs/pretend.pdf", "header"),
        ] {
            assert!(
                store.prepare(request(&scratch.0, relative, key)).is_err(),
                "invalid source {key} was accepted"
            );
        }
        assert!(store.list().unwrap().is_empty());
        assert_eq!(store.usage().unwrap().staging_bytes, 0);
        assert_eq!(store.usage().unwrap().stored_items, 0);
        assert!(store.scan_locked().unwrap().manifests.is_empty());
    }

    #[test]
    fn prepared_artifact_is_hidden_until_delivery_commit() {
        let scratch = Scratch::new();
        fs::write(
            scratch.0.join("workspace/outputs/report.csv"),
            b"a,b\n1,2\n",
        )
        .unwrap();
        let store = store(&scratch.0);
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.csv", "report"))
            .unwrap();
        assert!(store.list().unwrap().is_empty());
        store.commit_delivery(&commit(&receipt)).unwrap();
        let rows = store.list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].manifest.receipt.artifact_id, receipt.artifact_id);
        assert_eq!(
            store.read_payload(&receipt.artifact_id).unwrap(),
            b"a,b\n1,2\n"
        );
    }

    #[test]
    fn same_delivery_key_reuses_identical_content_and_rejects_changes() {
        let scratch = Scratch::new();
        let path = scratch.0.join("workspace/outputs/report.md");
        fs::write(&path, b"# report\n").unwrap();
        let store = store(&scratch.0);
        let first = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        let retry = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        assert_eq!(first, retry);
        fs::write(path, b"# changed\n").unwrap();
        assert!(
            store
                .prepare(request(&scratch.0, "outputs/report.md", "report"))
                .is_err()
        );
    }

    #[test]
    fn rejects_traversal_and_symlinked_sources() {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        assert!(
            store
                .prepare(request(&scratch.0, "../outside", "bad"))
                .is_err()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let external = scratch.0.join("secret.md");
            fs::write(&external, b"secret").unwrap();
            symlink(&external, scratch.0.join("workspace/outputs/link.md"))
                .unwrap();
            assert!(
                store
                    .prepare(request(&scratch.0, "outputs/link.md", "link"))
                    .is_err()
            );
        }
    }

    #[test]
    fn preferences_and_tombstones_survive_reopen_without_resurrection() {
        let scratch = Scratch::new();
        fs::write(scratch.0.join("workspace/outputs/report.md"), b"# report\n")
            .unwrap();
        let store = store(&scratch.0);
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        store.commit_delivery(&commit(&receipt)).unwrap();
        store.set_favorite(&receipt.artifact_id, true).unwrap();
        store.trash(&receipt.artifact_id).unwrap();
        store.permanently_delete(&receipt.artifact_id).unwrap();
        drop(store);
        let reopened =
            ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
        assert!(reopened.list().unwrap().is_empty());
        assert!(reopened.read_payload(&receipt.artifact_id).is_err());
    }

    #[test]
    fn permanent_deletion_rejects_active_files_and_late_confirmations_after_restore()
     {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(
            scratch.0.join("workspace/outputs/report.md"),
            b"protected report",
        )
        .unwrap();
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        store.commit_delivery(&commit(&receipt)).unwrap();
        let original_manifest = fs::read(
            store.item_dir(&receipt.artifact_id).join("manifest.json"),
        )
        .unwrap();
        assert!(store.permanently_delete(&receipt.artifact_id).is_err());
        assert_eq!(
            store.read_payload(&receipt.artifact_id).unwrap(),
            b"protected report"
        );
        assert_eq!(
            fs::read(
                store.item_dir(&receipt.artifact_id).join("manifest.json")
            )
            .unwrap(),
            original_manifest
        );
        store.trash(&receipt.artifact_id).unwrap();
        store.restore_from_trash(&receipt.artifact_id).unwrap();
        // A dialog opened while trashed may resolve after a separate restore.
        assert!(store.permanently_delete(&receipt.artifact_id).is_err());
        let restored = store.get(&receipt.artifact_id).unwrap().unwrap();
        assert_eq!(
            restored.manifest.publication_state,
            PublicationState::Committed
        );
        assert!(restored.preference.trashed_at_ms.is_none());
        assert_eq!(
            store.read_payload(&receipt.artifact_id).unwrap(),
            b"protected report"
        );
        assert_eq!(
            fs::read(
                store.item_dir(&receipt.artifact_id).join("manifest.json")
            )
            .unwrap(),
            original_manifest
        );
        store.trash(&receipt.artifact_id).unwrap();
        store.permanently_delete(&receipt.artifact_id).unwrap();
        let tombstone_path =
            store.item_dir(&receipt.artifact_id).join("manifest.json");
        let tombstone = fs::read(&tombstone_path).unwrap();
        assert_eq!(
            store
                .get(&receipt.artifact_id)
                .unwrap()
                .unwrap()
                .manifest
                .publication_state,
            PublicationState::Deleted
        );
        assert!(!store.payload_path(&receipt.artifact_id).exists());
        store.permanently_delete(&receipt.artifact_id).unwrap();
        assert_eq!(fs::read(tombstone_path).unwrap(), tombstone);
    }

    #[test]
    fn permanent_deletion_requires_a_delivery_marker_and_never_promotes_prepared_files()
     {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(
            scratch.0.join("workspace/outputs/report.md"),
            b"prepared report",
        )
        .unwrap();
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        let staging = store.staging_dir(
            &receipt.session_id,
            &receipt.submission_id,
            &receipt.artifact_id,
        );
        fs::rename(&staging, store.item_dir(&receipt.artifact_id)).unwrap();
        fs::rename(
            store.item_dir(&receipt.artifact_id).join("prepared.json"),
            store.item_dir(&receipt.artifact_id).join("manifest.json"),
        )
        .unwrap();
        let path = store.item_dir(&receipt.artifact_id).join("manifest.json");
        let prepared_bytes = fs::read(&path).unwrap();
        assert!(store.permanently_delete(&receipt.artifact_id).is_err());
        assert_eq!(fs::read(&path).unwrap(), prepared_bytes);
        let mut manifest = super::read_manifest(&path).unwrap();
        manifest.publication_state = PublicationState::Committed;
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let unmatched_bytes = fs::read(&path).unwrap();
        assert!(store.permanently_delete(&receipt.artifact_id).is_err());
        assert_eq!(fs::read(&path).unwrap(), unmatched_bytes);
        assert_eq!(
            fs::read(store.payload_path(&receipt.artifact_id)).unwrap(),
            b"prepared report"
        );
    }

    #[test]
    fn orphaned_partial_snapshots_and_view_copies_are_charged() {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::create_dir_all(store.root().join("staging/orphan/turn/file"))
            .unwrap();
        fs::write(
            store.root().join("staging/orphan/turn/file/payload"),
            b"partial",
        )
        .unwrap();
        fs::write(scratch.0.join("workspace/outputs/report.md"), b"# report\n")
            .unwrap();
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        store.commit_delivery(&commit(&receipt)).unwrap();
        let view = store.create_view_copy(&receipt.artifact_id).unwrap();
        let usage = store.usage().unwrap();
        assert_eq!(usage.staging_bytes, 7);
        assert_eq!(usage.stored_bytes, receipt.size_bytes);
        assert_eq!(usage.view_bytes, receipt.size_bytes);
        assert_eq!(usage.stored_items, 2);
        fs::write(&view.path, b"external edit").unwrap();
        assert_eq!(
            store.read_payload(&receipt.artifact_id).unwrap(),
            b"# report\n"
        );
        store.remove_view_copy(&view.request_id).unwrap();
        assert_eq!(store.usage().unwrap().view_bytes, 0);
    }

    #[test]
    fn partial_group_commit_is_invisible_and_recovers_as_a_whole() {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(scratch.0.join("workspace/outputs/one.md"), b"one").unwrap();
        fs::write(scratch.0.join("workspace/outputs/two.md"), b"two").unwrap();
        let one = store
            .prepare(request(&scratch.0, "outputs/one.md", "one"))
            .unwrap();
        let two = store
            .prepare(request(&scratch.0, "outputs/two.md", "two"))
            .unwrap();
        fs::rename(
            store.staging_dir(
                &one.session_id,
                &one.submission_id,
                &one.artifact_id,
            ),
            store.item_dir(&one.artifact_id),
        )
        .unwrap();
        assert!(store.list().unwrap().is_empty());
        let mut proof = commit(&one);
        proof.artifact_ids.push(two.artifact_id.clone());
        store.commit_delivery(&proof).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);
        proof.committed_at_ms += 1;
        proof.origin_title_snapshot = "later source title".into();
        store.commit_delivery(&proof).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn unknown_schema_is_preserved_and_does_not_hide_healthy_records() {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(scratch.0.join("workspace/outputs/report.md"), b"# report\n")
            .unwrap();
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        store.commit_delivery(&commit(&receipt)).unwrap();
        let bad = store.root().join("items/future");
        fs::create_dir(&bad).unwrap();
        let mut future =
            store.get(&receipt.artifact_id).unwrap().unwrap().manifest;
        future.schema_version = 999;
        future.receipt.artifact_id = "future".into();
        let bytes = serde_json::to_vec(&future).unwrap();
        fs::write(bad.join("manifest.json"), &bytes).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(!store.diagnostics().unwrap().is_empty());
        assert!(store.permanently_delete("future").is_err());
        assert_eq!(fs::read(bad.join("manifest.json")).unwrap(), bytes);
    }

    #[test]
    fn delete_pending_retries_on_open_and_tombstone_metadata_remains_queryable()
    {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(scratch.0.join("workspace/outputs/report.md"), b"# report\n")
            .unwrap();
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        store.commit_delivery(&commit(&receipt)).unwrap();
        store.trash(&receipt.artifact_id).unwrap();
        let mut manifest =
            store.get(&receipt.artifact_id).unwrap().unwrap().manifest;
        manifest.publication_state = PublicationState::DeletePending;
        fs::write(
            store.item_dir(&receipt.artifact_id).join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        drop(store);
        let reopened =
            ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
        let deleted = reopened.get(&receipt.artifact_id).unwrap().unwrap();
        assert_eq!(
            deleted.manifest.publication_state,
            PublicationState::Deleted
        );
        assert!(!deleted.available);
        assert_eq!(reopened.usage().unwrap().stored_bytes, 0);
        reopened.commit_delivery(&commit(&receipt)).unwrap();
        assert!(reopened.list().unwrap().is_empty());
    }

    fn age_prepared(
        store: &ArtifactStore,
        receipt: &flowy_agent_core::ArtifactReceipt,
        age_ms: u64,
    ) -> PathBuf {
        let directory = store.staging_dir(
            &receipt.session_id,
            &receipt.submission_id,
            &receipt.artifact_id,
        );
        let path = directory.join("prepared.json");
        let mut manifest = super::read_manifest(&path).unwrap();
        manifest.receipt.created_at_ms = super::now_ms().saturating_sub(age_ms);
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        directory
    }

    #[test]
    fn cleanup_requires_readable_history_retains_seven_days_and_protects_successful_proofs()
     {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(
            scratch.0.join("workspace/outputs/report.md"),
            b"retained data",
        )
        .unwrap();
        let old = store
            .prepare(request(&scratch.0, "outputs/report.md", "old-failure"))
            .unwrap();
        let recent = store
            .prepare(request(&scratch.0, "outputs/report.md", "recent-failure"))
            .unwrap();
        let protected = store
            .prepare(request(
                &scratch.0,
                "outputs/report.md",
                "successful-history",
            ))
            .unwrap();
        let old_dir =
            age_prepared(&store, &old, PREPARED_RETENTION_MS + 60_000);
        let recent_dir = age_prepared(
            &store,
            &recent,
            PREPARED_RETENTION_MS - 24 * 60 * 60 * 1_000,
        );
        let protected_dir =
            age_prepared(&store, &protected, PREPARED_RETENTION_MS + 60_000);
        let raw =
            store.staging_dir("raw-session", "raw-submission", "unproved-copy");
        let raw_protected =
            store.staging_dir("raw-session", "raw-submission", "proved-copy");
        for directory in [&raw, &raw_protected] {
            fs::create_dir_all(directory).unwrap();
            fs::write(directory.join("payload"), b"incomplete copy").unwrap();
        }
        let mut proof = commit(&protected);
        proof.artifact_ids.push("proved-copy".into());

        // A corrupt/unreadable history cannot authorize even raw-copy cleanup.
        store
            .cleanup_uncommitted(std::slice::from_ref(&proof), false)
            .unwrap();
        assert!(old_dir.exists());
        assert!(raw.exists());
        store
            .cleanup_uncommitted(std::slice::from_ref(&proof), true)
            .unwrap();
        assert!(!old_dir.exists());
        assert!(!raw.exists());
        assert!(recent_dir.join("payload").is_file());
        assert!(protected_dir.join("payload").is_file());
        assert!(raw_protected.join("payload").is_file());
    }

    #[test]
    fn cleanup_never_ages_out_committed_payloads() {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(
            scratch.0.join("workspace/outputs/report.md"),
            b"committed data",
        )
        .unwrap();
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        age_prepared(&store, &receipt, PREPARED_RETENTION_MS + 60_000);
        store.commit_delivery(&commit(&receipt)).unwrap();
        // Independent markers protect files even after their session is gone.
        store.cleanup_uncommitted(&[], true).unwrap();
        assert_eq!(
            store.read_payload(&receipt.artifact_id).unwrap(),
            b"committed data"
        );
    }

    #[test]
    fn corrupt_marker_isolates_its_group_but_blocks_cleanup_and_new_metadata() {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(scratch.0.join("workspace/outputs/report.md"), b"real data")
            .unwrap();
        let bad = store
            .prepare(request(&scratch.0, "outputs/report.md", "first"))
            .unwrap();
        store.commit_delivery(&commit(&bad)).unwrap();
        let mut second_request =
            request(&scratch.0, "outputs/report.md", "second");
        second_request.submission_id = "submission-2".into();
        let healthy = store.prepare(second_request).unwrap();
        store.commit_delivery(&commit(&healthy)).unwrap();
        let raw =
            store.staging_dir("raw-session", "raw-submission", "raw-copy");
        fs::create_dir_all(&raw).unwrap();
        fs::write(raw.join("payload"), b"partial").unwrap();
        let marker_path =
            store.delivery_path(&bad.session_id, &bad.submission_id);
        fs::write(&marker_path, b"{broken marker").unwrap();
        let records = store.list().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].manifest.receipt.artifact_id,
            healthy.artifact_id
        );
        assert_eq!(
            store.read_payload(&healthy.artifact_id).unwrap(),
            b"real data"
        );
        assert!(store.read_payload(&bad.artifact_id).is_err());
        assert!(!store.diagnostics().unwrap().is_empty());
        assert!(store.cleanup_uncommitted(&[], true).is_err());
        assert!(raw.join("payload").is_file());
        assert!(store.set_favorite(&healthy.artifact_id, true).is_err());
        assert_eq!(fs::read(marker_path).unwrap(), b"{broken marker");
    }

    #[test]
    fn future_manifest_preflight_prevents_partial_cleanup() {
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        fs::write(
            scratch.0.join("workspace/outputs/report.md"),
            b"prepared data",
        )
        .unwrap();
        let receipt = store
            .prepare(request(&scratch.0, "outputs/report.md", "report"))
            .unwrap();
        let directory =
            age_prepared(&store, &receipt, PREPARED_RETENTION_MS + 60_000);
        let path = directory.join("prepared.json");
        let mut manifest = super::read_manifest(&path).unwrap();
        manifest.schema_version = 999;
        let bytes = serde_json::to_vec(&manifest).unwrap();
        fs::write(&path, &bytes).unwrap();
        let raw =
            store.staging_dir("raw-session", "raw-submission", "raw-copy");
        fs::create_dir_all(&raw).unwrap();
        fs::write(raw.join("payload"), b"partial").unwrap();
        assert!(store.cleanup_uncommitted(&[], true).is_err());
        assert!(raw.join("payload").is_file());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn atomic_metadata_replacement_updates_an_existing_destination() {
        let scratch = Scratch::new();
        let path = scratch.0.join("preferences.json");
        fs::write(&path, b"old bytes").unwrap();
        super::atomic_write(&path, b"new bytes").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"new bytes");
        assert!(fs::read_dir(&scratch.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".artifact-")
        }));
    }

    #[cfg(windows)]
    #[test]
    fn occupied_metadata_replacement_keeps_original_bytes_and_no_backup_gap() {
        use std::os::windows::fs::OpenOptionsExt;
        let scratch = Scratch::new();
        let path = scratch.0.join("preferences.json");
        fs::write(&path, b"old bytes").unwrap();
        let occupied = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(super::atomic_write(&path, b"new bytes").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old bytes");
        assert!(fs::read_dir(&scratch.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".artifact-")
        }));
        drop(occupied);
        super::atomic_write(&path, b"new bytes").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"new bytes");
    }

    #[cfg(windows)]
    #[test]
    fn occupied_uncommitted_cleanup_keeps_actual_quota_until_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        let scratch = Scratch::new();
        let store = store(&scratch.0);
        let raw =
            store.staging_dir("raw-session", "raw-submission", "raw-copy");
        fs::create_dir_all(&raw).unwrap();
        let payload = raw.join("payload");
        fs::write(&payload, b"partial").unwrap();
        let occupied = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&payload)
            .unwrap();
        assert!(store.cleanup_uncommitted(&[], true).is_err());
        assert_eq!(store.usage().unwrap().staging_bytes, 7);
        assert!(!store.diagnostics().unwrap().is_empty());
        assert_eq!(fs::read(&payload).unwrap(), b"partial");
        drop(occupied);
        store.cleanup_uncommitted(&[], true).unwrap();
        assert_eq!(store.usage().unwrap().staging_bytes, 0);
    }
}
