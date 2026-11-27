//! Durable snapshots for explicitly published conversation artifacts.
//!
//! Prepared snapshots are invisible until the engine has durably saved the
//! whole-turn `DeliveryCommit`. The independent marker is written last and is
//! the only source of list visibility.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
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
pub(in crate::artifact_store) const MAX_SCANNED_ENTRIES: usize = 10_000;
pub(in crate::artifact_store) const PREPARED_RETENTION_MS: u64 =
    7 * 24 * 60 * 60 * 1_000;
pub(in crate::artifact_store) const VIEW_COPY_RETENTION_MS: u64 =
    3 * 24 * 60 * 60 * 1_000;
pub(in crate::artifact_store) const MAX_RETAINED_VIEW_COPIES: usize = 5;

mod deletion_plan;
mod io_helpers;
mod views_gc;

pub use deletion_plan::SessionDeletionState;
pub use io_helpers::can_open_with_default_app;
use io_helpers::{
    atomic_write, copy_and_hash, hex, invalid, is_windows_reparse_point,
    media_type, now_ms, origin_may_own_artifacts, random_id, read_limited_json,
    read_manifest, reject_link, safe_display_name, sha256_hex, snapshot_file,
    validate_delivery_key, validate_identifier, validate_known_header,
    validate_relative_path, write_new_synced,
};

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
    pub(in crate::artifact_store) inner: Arc<StoreInner>,
}

pub(in crate::artifact_store) struct StoreInner {
    pub(in crate::artifact_store) root: PathBuf,
    serial: Mutex<()>,
    pub(in crate::artifact_store) cleanup_diagnostics: Mutex<Vec<String>>,
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
pub(in crate::artifact_store) struct ViewCopyMeta {
    pub(in crate::artifact_store) request_id: String,
    pub(in crate::artifact_store) artifact_id: String,
    pub(in crate::artifact_store) file_name: String,
    pub(in crate::artifact_store) sha256: String,
    pub(in crate::artifact_store) created_at_ms: u64,
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

    pub(in crate::artifact_store) fn list_locked(
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

    pub(in crate::artifact_store) fn read_payload_locked(
        &self,
        artifact_id: &str,
    ) -> io::Result<Vec<u8>> {
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

    pub(in crate::artifact_store) fn commit_delivery(
        &self,
        commit: &DeliveryCommit,
    ) -> io::Result<()> {
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

    pub(in crate::artifact_store) fn read_delivery_marker(
        &self,
        path: &Path,
    ) -> io::Result<DeliveryMarker> {
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

    pub(in crate::artifact_store) fn committed_ids_locked(
        &self,
    ) -> io::Result<BTreeSet<String>> {
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

    pub(in crate::artifact_store) fn scan_locked(&self) -> io::Result<Scan> {
        self.scan_with_diagnostics_locked(true)
    }

    pub(in crate::artifact_store) fn scan_with_diagnostics_locked(
        &self,
        strict: bool,
    ) -> io::Result<Scan> {
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
    pub(in crate::artifact_store) fn preferences_locked(
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

    pub(in crate::artifact_store) fn save_preferences_locked(
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

    pub(in crate::artifact_store) fn write_metadata_locked(
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

    pub(in crate::artifact_store) fn lock(&self) -> MutexGuard<'_, ()> {
        self.inner
            .serial
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(in crate::artifact_store) fn staging_dir(
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
    pub(in crate::artifact_store) fn item_dir(
        &self,
        artifact: &str,
    ) -> PathBuf {
        self.inner.root.join("items").join(artifact)
    }
    pub(in crate::artifact_store) fn payload_path(
        &self,
        artifact: &str,
    ) -> PathBuf {
        self.item_dir(artifact).join("payload")
    }
    pub(in crate::artifact_store) fn delivery_path(
        &self,
        session: &str,
        submission: &str,
    ) -> PathBuf {
        let key = format!("{session}\0{submission}");
        self.inner
            .root
            .join("deliveries")
            .join(format!("{}.json", hex(&Sha256::digest(key.as_bytes()))))
    }
    pub(in crate::artifact_store) fn session_deletion_path(
        &self,
        session_id: &str,
    ) -> PathBuf {
        self.inner
            .root
            .join("session-deletions")
            .join(format!("{session_id}.json"))
    }
    pub(in crate::artifact_store) fn source_deletion_path(
        &self,
        session_id: &str,
    ) -> PathBuf {
        self.inner
            .root
            .join("sources-deleted")
            .join(format!("{session_id}.json"))
    }
    pub(in crate::artifact_store) fn view_meta_path(
        &self,
        request_id: &str,
    ) -> PathBuf {
        self.inner
            .root
            .join("views-meta")
            .join(format!("{request_id}.json"))
    }
}

#[derive(Default)]
pub(in crate::artifact_store) struct Scan {
    pub(in crate::artifact_store) manifests: Vec<ArtifactManifest>,
    pub(in crate::artifact_store) payload_bytes: u64,
    pub(in crate::artifact_store) payload_files: usize,
    pub(in crate::artifact_store) metadata_bytes: u64,
    pub(in crate::artifact_store) entry_count: usize,
    pub(in crate::artifact_store) diagnostics: Vec<String>,
    pub(in crate::artifact_store) stored_bytes: u64,
    pub(in crate::artifact_store) staging_bytes: u64,
    pub(in crate::artifact_store) view_bytes: u64,
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

#[cfg(test)]
mod tests;
