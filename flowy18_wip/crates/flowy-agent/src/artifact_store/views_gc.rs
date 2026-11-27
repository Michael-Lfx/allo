//! Extension-preserving view copies and uncommitted staging / stale-view garbage collection.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

use flowy_agent_core::DeliveryCommit;

use super::io_helpers::{
    atomic_write, can_open_with_default_app, invalid, is_windows_reparse_point,
    now_ms, random_id, read_limited_json, read_manifest, reject_link,
    remove_file_if_exists, sha256_hex, snapshot_file, validate_identifier,
    write_new_synced,
};
use super::{
    ArtifactStore, MAX_MANIFEST_BYTES, MAX_RETAINED_VIEW_COPIES,
    MAX_SCANNED_ENTRIES, MAX_STORE_BYTES, PREPARED_RETENTION_MS,
    PublicationState, VIEW_COPY_RETENTION_MS, ViewCopy, ViewCopyMeta,
};

impl ArtifactStore {
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
    pub(super) fn cleanup_stale_view_copies_at(
        &self,
        now_ms: u64,
    ) -> io::Result<usize> {
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
}

struct ManagedViewCopy {
    meta: ViewCopyMeta,
    dir: PathBuf,
    file_path: PathBuf,
    meta_path: PathBuf,
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
