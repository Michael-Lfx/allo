//! Owner-aware result visibility (PA-A11).
//!
//! Every UI-facing query, preview, export, open, favorite and navigation entry
//! rechecks the host-assigned viewer here. Automation sessions and artifacts
//! are visible only to the account recorded as their durable owner; a missing
//! owner or malformed identity is quarantined. Legacy and foreground content
//! without an owner keeps its existing rules and is never auto-claimed by the
//! account that signs in next. Host-internal recovery (detached reconcile and
//! commit retry) does not pass through this gate.

use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;

use flowy_agent_core::{ExecutionMetadata, ExecutionOwner, ExecutionSource};

use super::{
    AgentHandle, AgentUpdate, ArtifactRecord, ArtifactStore, lock,
    persist_index,
};
use crate::artifact_store::{ArtifactUsage, ViewCopy};

/// The single visibility rule shared by sessions and artifacts.
#[must_use]
pub fn result_visible(
    execution: &ExecutionMetadata,
    viewer: Option<&ExecutionOwner>,
) -> bool {
    if !execution.is_valid() {
        return false;
    }
    match (&execution.source, execution.owner.as_ref()) {
        // `is_valid` already requires an owner; keep the arm fail-closed.
        (ExecutionSource::Automation { .. }, None) => false,
        // Unowned legacy/foreground content keeps its previous behavior.
        (_, None) => true,
        (_, Some(owner)) => viewer == Some(owner),
    }
}

fn not_found() -> io::Error {
    // Do not distinguish "belongs to another account" from "missing".
    io::Error::new(io::ErrorKind::NotFound, "artifact not found")
}

fn unavailable() -> io::Error {
    io::Error::other("artifact store unavailable")
}

impl AgentHandle {
    /// Install the host's current account as the result viewer. Every call,
    /// including a same-account re-login, advances the viewer generation so
    /// reads started under the previous account can be discarded on arrival.
    /// An active session the new viewer cannot see is deselected.
    pub fn set_result_viewer(&self, owner: Option<ExecutionOwner>) -> u64 {
        let state = &self.inner.state;
        let _dispatch = lock(&state.dispatch);
        *lock(&state.result_viewer) = owner;
        let generation = state
            .result_viewer_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let active = lock(&state.active).clone();
        if let Some(id) = active
            && !self.session_visible(&id)
        {
            // Fail closed in memory even if the index write fails.
            *lock(&state.active) = None;
            *lock(&state.workspace_for_new_sessions) = None;
            if persist_index(state).is_err() {
                state.report_storage_error();
            }
            state.context_generation.fetch_add(1, Ordering::AcqRel);
        }
        state.push_update(AgentUpdate::Sessions);
        state.push_update(AgentUpdate::Artifacts);
        generation
    }

    /// Compare before applying any asynchronously loaded result.
    #[must_use]
    pub fn result_viewer_generation(&self) -> u64 {
        self.inner
            .state
            .result_viewer_generation
            .load(Ordering::Acquire)
    }

    pub(super) fn result_viewer(&self) -> Option<ExecutionOwner> {
        lock(&self.inner.state.result_viewer).clone()
    }

    /// Live projection first, then structured history. Unknown IDs are not
    /// visible, so guessing an ID never reaches another account's session.
    /// Callers must not hold the `live` or engine lock.
    pub(super) fn session_visible(&self, session_id: &str) -> bool {
        let viewer = self.result_viewer();
        let live = lock(&self.inner.state.live)
            .get(session_id)
            .map(|live| result_visible(&live.execution, viewer.as_ref()));
        if let Some(visible) = live {
            return visible;
        }
        let engine = lock(&self.inner.engine).clone();
        engine
            .as_ref()
            .and_then(|engine| {
                engine.with_session(session_id, |record| {
                    result_visible(&record.execution, viewer.as_ref())
                })
            })
            .unwrap_or(false)
    }

    pub(super) fn artifact_visible(&self, record: &ArtifactRecord) -> bool {
        result_visible(
            &record.manifest.execution,
            self.result_viewer().as_ref(),
        )
    }

    fn raw_artifact_store(&self) -> io::Result<ArtifactStore> {
        self.inner
            .state
            .artifact_store
            .get()
            .cloned()
            .ok_or_else(unavailable)
    }

    /// Resolve the store only after this exact artifact passes the viewer
    /// check. Manifest execution identity is immutable after preparation.
    pub(super) fn authorized_artifact_store(
        &self,
        artifact_id: &str,
    ) -> io::Result<ArtifactStore> {
        let store = self.raw_artifact_store()?;
        if !store
            .get(artifact_id)?
            .is_some_and(|record| self.artifact_visible(&record))
        {
            return Err(not_found());
        }
        Ok(store)
    }

    /// Save a verified snapshot to a user-selected path (visible items only).
    pub fn copy_artifact_to(
        &self,
        artifact_id: &str,
        destination: &Path,
    ) -> io::Result<()> {
        self.authorized_artifact_store(artifact_id)?
            .copy_to(artifact_id, destination)
    }

    /// Private extension-preserving copy for the system opener.
    pub fn create_artifact_view_copy(
        &self,
        artifact_id: &str,
    ) -> io::Result<ViewCopy> {
        self.authorized_artifact_store(artifact_id)?
            .create_view_copy(artifact_id)
    }

    /// Aggregate quota numbers carry no per-item content or identity.
    pub fn artifact_usage(&self) -> io::Result<ArtifactUsage> {
        self.raw_artifact_store()?.usage()
    }

    /// Whether the catalog has integrity issues; details stay in logs.
    pub fn artifact_catalog_has_issues(&self) -> bool {
        self.raw_artifact_store()
            .and_then(|store| store.diagnostics())
            .map_or(true, |issues| !issues.is_empty())
    }
}

#[cfg(test)]
#[path = "visibility_tests.rs"]
mod tests;
