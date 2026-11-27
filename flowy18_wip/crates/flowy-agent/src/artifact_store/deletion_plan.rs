//! Pending-deletion recovery, session deletion plans, source tombstones,
//! and superseded conversation-import retirement.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::io_helpers::{
    invalid, now_ms, read_limited_json, read_manifest, reject_link,
    remove_file_if_exists, validate_identifier,
};
use super::{
    ArtifactManifest, ArtifactStore, MAX_MANIFEST_BYTES, PublicationState,
};

impl ArtifactStore {
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

    pub(super) fn prune_superseded_conversation_imports(
        &self,
    ) -> io::Result<()> {
        let _guard = self.lock();
        self.prune_superseded_conversation_imports_locked()
    }

    pub(super) fn prune_superseded_conversation_imports_locked(
        &self,
    ) -> io::Result<()> {
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

    pub(super) fn session_deletion_ids_locked(
        &self,
    ) -> io::Result<BTreeSet<String>> {
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
