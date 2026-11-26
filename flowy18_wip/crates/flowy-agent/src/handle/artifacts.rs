//! Host-side artifact catalog, retry-delivery, and override delivery flows.

use super::{
    AgentHandle, AgentUpdate, ArtifactRecord, ArtifactStore, BTreeSet,
    ChatLine, DeliveryCommit, Path, PathBuf, SessionOrigin, SessionRole,
    TurnStatus, UiState, apply_delivery_commit_to_live, confined_to,
    confirm_commit_audit, lock, log_storage_io_error, next_stamp, now_ms,
    persist_index, reconcile_partial_work_after_override,
};
use crate::artifact_store::PublicationState;

impl AgentHandle {
    /// The host-side artifact catalog API. A missing store means startup
    /// storage initialization failed; callers should surface a storage error.
    pub fn artifact_store(&self) -> Result<ArtifactStore, &'static str> {
        self.inner
            .state
            .artifact_store
            .get()
            .cloned()
            .ok_or("notice.session_storage_failed")
    }

    /// A confirmed source deletion is separate from a temporarily missing
    /// history record. Store reads happen on the caller's background executor.
    pub fn source_deleted(&self, session_id: &str) -> bool {
        if *lock(&self.inner.state.load_failed) {
            return false;
        }
        if lock(&self.inner.engine)
            .as_ref()
            .is_some_and(|engine| engine.session(session_id).is_some())
        {
            return false;
        }
        self.inner
            .state
            .artifact_store
            .get()
            .and_then(|store| store.source_deleted(session_id).ok())
            .unwrap_or(false)
    }

    #[must_use]
    pub fn source_available(&self, session_id: &str) -> bool {
        if *lock(&self.inner.state.load_failed) {
            return false;
        }
        let record = lock(&self.inner.engine)
            .as_ref()
            .and_then(|engine| engine.session(session_id));
        record.is_some_and(|record| {
            !*lock(&self.inner.state.history_unavailable)
                || record.origin != SessionOrigin::Unknown
        })
    }

    /// Remove a conversation and every artifact delivered by it. A durable
    /// session-deletion plan spans the history/index commit and idempotent
    /// artifact cleanup so a failed cleanup cannot resurrect or orphan files.
    pub fn delete_session(&self, session_id: &str) -> Result<(), &'static str> {
        if self.is_initializing() {
            return Err("library.loading");
        }
        let _dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let record = engine.session(session_id).ok_or("notice.agent_failed")?;
        if record.role == flowy_agent_core::SessionRole::Worker {
            return Err("notice.agent_failed");
        }
        let children = engine.child_sessions(session_id);
        let child_ids: Vec<String> =
            children.iter().map(|c| c.id.clone()).collect();
        self.inner.settle_finishing_fence(&engine, session_id);
        for cid in &child_ids {
            self.inner.settle_finishing_fence(&engine, cid);
        }
        let any_claimed = {
            let claims = lock(&self.inner.state.active_claims);
            claims.values().any(|claim| {
                claim.session_id == session_id
                    || child_ids.iter().any(|cid| cid == &claim.session_id)
            })
        };
        let any_busy = record.status.is_busy()
            || !record.pending_router_followups.is_empty()
            || any_claimed
            || engine.is_active(session_id)
            || lock(&self.inner.state.live)
                .get(session_id)
                .is_some_and(|s| s.status.is_busy())
            || children.iter().any(|child| {
                child.status.is_busy()
                    || engine.is_active(&child.id)
                    || lock(&self.inner.state.live)
                        .get(&child.id)
                        .is_some_and(|s| s.status.is_busy())
            });
        if any_busy {
            return Err("notice.agent_busy");
        }
        let store = self.artifact_store()?;
        let artifact_ids = std::iter::once(&record)
            .chain(children.iter())
            .flat_map(|rec| rec.delivery_commits.iter())
            .flat_map(|commit| commit.artifact_ids.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let submission_ids = std::iter::once(&record)
            .chain(children.iter())
            .flat_map(|rec| rec.delivery_commits.iter())
            .map(|commit| commit.submission_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        store
            .prepare_cascade_session_deletion(
                session_id,
                &child_ids,
                &artifact_ids,
                &submission_ids,
            )
            .map_err(|error| {
                log_storage_io_error(
                    "prepare session deletion",
                    &self.inner.state.root.join("artifacts"),
                    &error,
                );
                self.inner.state.report_storage_error();
                "notice.session_storage_failed"
            })?;

        // Stop title requests and clear any buffered Router inbox events
        // before removing parent and child sessions so no announcement flush
        // can race with deletion.
        lock(&self.inner.state.router_inboxes).remove(session_id);
        {
            let live = lock(&self.inner.state.live);
            if let Some(session) = live.get(session_id) {
                session.title_signal.cancel();
            }
            for cid in &child_ids {
                if let Some(session) = live.get(cid) {
                    session.title_signal.cancel();
                }
            }
            drop(live);
        }

        if child_ids.is_empty() {
            if let Err(error) = engine.remove_session(session_id) {
                log_storage_io_error(
                    "remove structured session",
                    &flowy_agent_core::SessionIndex::path(
                        &self.inner.state.root,
                    ),
                    &error,
                );
                if let Err(cancel_error) =
                    store.cancel_session_deletion(session_id)
                {
                    log_storage_io_error(
                        "cancel prepared session deletion",
                        &self.inner.state.root.join("artifacts"),
                        &cancel_error,
                    );
                    self.inner.state.report_storage_error();
                }
                return Err("notice.session_storage_failed");
            }
            let mut live = lock(&self.inner.state.live);
            let removed = live.remove(session_id);
            drop(live);
            let previous_active = lock(&self.inner.state.active).clone();
            if previous_active.as_deref() == Some(session_id) {
                *lock(&self.inner.state.active) = None;
            }
            if let Err(error) = persist_index(&self.inner.state) {
                if engine.insert_session(record).is_err() {
                    self.inner.state.report_storage_error();
                    self.inner.state.push_update(AgentUpdate::Sessions);
                    self.inner.state.push_update(AgentUpdate::Artifacts);
                    return Err("notice.session_storage_failed");
                }
                if engine.set_active(previous_active.clone()).is_err() {
                    self.inner.state.report_storage_error();
                }
                if let Some(removed) = removed {
                    lock(&self.inner.state.live)
                        .insert(session_id.to_owned(), removed);
                }
                *lock(&self.inner.state.active) = previous_active;
                if let Err(cancel_error) =
                    store.cancel_session_deletion(session_id)
                {
                    log_storage_io_error(
                        "cancel prepared session deletion",
                        &self.inner.state.root.join("artifacts"),
                        &cancel_error,
                    );
                    self.inner.state.report_storage_error();
                }
                self.inner.state.report_storage_error();
                return Err(error);
            }
            super::observer::cancel_receipts_for_sessions(
                &self.inner.state,
                std::iter::once(session_id),
            );
            lock(&self.inner.state.router_inboxes).remove(session_id);
        } else {
            super::observer::cancel_receipts_for_sessions(
                &self.inner.state,
                std::iter::once(session_id)
                    .chain(child_ids.iter().map(String::as_str)),
            );
            for (idx, cid) in child_ids.iter().enumerate() {
                if let Err(error) = engine.remove_session(cid) {
                    log_storage_io_error(
                        "remove child worker session",
                        &flowy_agent_core::SessionIndex::path(
                            &self.inner.state.root,
                        ),
                        &error,
                    );
                    self.inner.state.report_storage_error();
                    return Err("notice.session_storage_failed");
                }
                if idx == 0 {
                    #[cfg(test)]
                    {
                        let hook = lock(
                            &self.inner.state.after_first_child_deleted_hook,
                        )
                        .clone();
                        if let Some(hook) = hook
                            && let Err(injected) = hook(session_id, cid)
                        {
                            return Err(injected);
                        }
                    }
                }
            }
            if let Err(error) = engine.remove_session(session_id) {
                log_storage_io_error(
                    "remove parent router session",
                    &flowy_agent_core::SessionIndex::path(
                        &self.inner.state.root,
                    ),
                    &error,
                );
                self.inner.state.report_storage_error();
                return Err("notice.session_storage_failed");
            }
            {
                let mut live = lock(&self.inner.state.live);
                for cid in &child_ids {
                    live.remove(cid);
                }
                live.remove(session_id);
                drop(live);
            }
            lock(&self.inner.state.router_inboxes).remove(session_id);
            let previous_active = lock(&self.inner.state.active).clone();
            if previous_active.as_deref() == Some(session_id)
                || previous_active
                    .as_deref()
                    .is_some_and(|a| child_ids.iter().any(|c| c == a))
            {
                *lock(&self.inner.state.active) = None;
            }
            if persist_index(&self.inner.state).is_err() {
                self.inner.state.report_storage_error();
            }
        }

        // The conversation removal is the logical deletion commit point. The
        // committed plan keeps artifacts hidden and makes physical cleanup
        // retryable even if the process exits midway.
        if let Err(error) = store.commit_session_deletion(session_id) {
            log_storage_io_error(
                "commit session deletion",
                &self.inner.state.root.join("artifacts"),
                &error,
            );
            self.inner.state.report_storage_error();
        } else if let Err(error) = store.cleanup_session_deletion(session_id) {
            log_storage_io_error(
                "clean up session deletion",
                &self.inner.state.root.join("artifacts"),
                &error,
            );
            self.inner.state.report_storage_error();
        } else if let Err(error) = store.complete_session_deletion(session_id) {
            log_storage_io_error(
                "complete session deletion",
                &self.inner.state.root.join("artifacts"),
                &error,
            );
            self.inner.state.report_storage_error();
        }
        self.inner.state.push_update(AgentUpdate::Sessions);
        self.inner.state.push_update(AgentUpdate::Artifacts);
        Ok(())
    }

    /// Retry the independent artifact-store marker for a durable delivery
    /// commit after the turn itself has already completed. The turn is never
    /// re-run and no new submission or artifact ID is created.
    pub fn retry_delivery(
        &self,
        session_id: Option<&str>,
    ) -> Result<(), &'static str> {
        let session_id = session_id.ok_or("chat.status_delivery_failed")?;
        let dispatch = lock(&self.inner.state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let record = engine
            .session(session_id)
            .ok_or("chat.status_delivery_failed")?;
        if record.delivery_commits.is_empty() {
            return Err("chat.status_delivery_failed");
        }
        if record.status.is_busy()
            || engine.is_active(session_id)
            || lock(&self.inner.state.live)
                .get(session_id)
                .is_some_and(|session| session.status.is_busy())
        {
            return Err("notice.agent_busy");
        }
        if !lock(&self.inner.state.live).get(session_id).is_some_and(
            |session| matches!(session.status, TurnStatus::DeliveryFailed),
        ) {
            return Err("chat.status_delivery_failed");
        }
        let store = self.artifact_store()?;
        if let Some(session) = lock(&self.inner.state.live).get_mut(session_id)
        {
            session.status = TurnStatus::SavingFiles;
            session.turn_started_at = Some(now_ms());
        }
        self.inner.state.stream_changed(session_id);
        let outcome = if let Err(error) =
            store.recover_commits(&record.delivery_commits)
        {
            log_storage_io_error(
                "retry delivered artifacts",
                &self.inner.state.root.join("artifacts"),
                &error,
            );
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.status = TurnStatus::DeliveryFailed;
                session.turn_started_at = None;
            }
            self.inner.state.stream_changed(session_id);
            Err("chat.status_delivery_failed")
        } else {
            for commit in &record.delivery_commits {
                confirm_commit_audit(&self.inner.state.root, &store, commit);
                apply_delivery_commit_to_live(&self.inner.state, commit);
            }
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.projection.lines.retain(|line| {
                    !matches!(
                        line,
                        ChatLine::Status { text }
                            if text == "chat.status_delivery_failed"
                    )
                });
                session.status = TurnStatus::Completed;
                session.turn_started_at = None;
                session.updated_at = now_ms();
            }
            if persist_index(&self.inner.state).is_err() {
                self.inner.state.report_storage_error();
            }
            self.inner.state.stream_changed(session_id);
            self.inner.state.push_update(AgentUpdate::Sessions);
            self.inner.state.push_update(AgentUpdate::Artifacts);
            Ok(())
        };
        drop(dispatch);
        // Entering `TurnStatus::SavingFiles` paused autonomous drain; resume
        // draining any waiting receipts now that the session is no longer busy.
        if self
            .inner
            .state
            .autonomous_drain
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.inner.kick_autonomous_drain();
        }
        outcome
    }

    pub fn artifacts(&self) -> Result<Vec<ArtifactRecord>, std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .list()
    }

    pub fn artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<ArtifactRecord>, std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .get(artifact_id)
    }

    pub fn set_artifact_favorite(
        &self,
        artifact_id: &str,
        favorite: bool,
    ) -> Result<(), std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .set_favorite(artifact_id, favorite)
    }

    pub fn trash_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<(), std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .trash(artifact_id)
    }

    pub fn restore_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<(), std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .restore_from_trash(artifact_id)
    }

    pub fn permanently_delete_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<(), std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .permanently_delete(artifact_id)
    }

    pub fn read_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<u8>, std::io::Error> {
        self.inner
            .state
            .artifact_store
            .get()
            .ok_or_else(|| std::io::Error::other("artifact store unavailable"))?
            .read_payload(artifact_id)
    }

    /// All artifact SHA-256 hashes delivered through an explicit user
    /// verification override (`deliver_override` + `deliver`), read in one pass.
    #[must_use]
    pub fn overridden_artifact_hashes(&self) -> BTreeSet<String> {
        crate::delivery::DeliveryStore::new(
            self.inner.state.root.join("delivery"),
        )
        .overridden_artifact_hashes()
        .unwrap_or_default()
    }

    /// Whether `sha256` was delivered through an explicit user verification
    /// override (`deliver_override` + `deliver` in `delivery/events.jsonl`).
    #[must_use]
    pub fn artifact_was_overridden(&self, sha256: &str) -> bool {
        !sha256.is_empty() && self.overridden_artifact_hashes().contains(sha256)
    }

    /// Explicitly deliver an artifact whose verification failed (`ok=false`),
    /// after the user confirmed the override in Studio (§8.3).
    pub fn override_deliver_artifact(
        &self,
        session_id: &str,
        report_id: &str,
        artifact_path: &str,
    ) -> Result<String, &'static str> {
        if self.is_initializing() {
            return Err("library.loading");
        }
        if report_id.trim().is_empty() {
            return Err("chat.publish.not_verified");
        }
        let state = &self.inner.state;
        let _dispatch = lock(&state.dispatch);
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let core_session =
            engine.session(session_id).ok_or("chat.publish.context")?;
        if core_session.origin != SessionOrigin::Home {
            return Err("chat.publish.home_only");
        }
        let session_workspace = core_session
            .workspace
            .as_ref()
            .ok_or("chat.publish.context")?;
        let store = state
            .artifact_store
            .get()
            .cloned()
            .ok_or("chat.publish.unavailable")?;
        if !confined_to(&session_workspace.root, artifact_path) {
            return Err("chat.publish.failed");
        }
        let full_path = if Path::new(artifact_path).is_absolute() {
            PathBuf::from(artifact_path)
        } else {
            session_workspace.root.join(artifact_path)
        };
        let source_root = full_path
            .parent()
            .ok_or("chat.publish.failed")?
            .to_path_buf();
        let file_name = full_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("chat.publish.failed")?
            .to_owned();
        let source_relative_path = {
            let resolved_root = super::guards::resolve_deepest_ancestor(
                &session_workspace.root,
            );
            let resolved_full =
                super::guards::resolve_deepest_ancestor(&full_path);
            resolved_full
                .strip_prefix(&resolved_root)
                .or_else(|_| full_path.strip_prefix(&session_workspace.root))
                .map_or_else(
                    |_| file_name.clone(),
                    |rel| rel.to_string_lossy().replace('\\', "/"),
                )
        };
        let delivery_key = format!("override-{report_id}");
        let gate =
            crate::delivery::DeliveryStore::new(state.root.join("delivery"));
        let now = now_ms();
        let current_bytes = crate::delivery::read_regular_file_bounded(
            &full_path,
            crate::MAX_ARTIFACT_BYTES,
        )
        .map_err(|_| "chat.publish.failed")?;
        let current_hash = crate::delivery::hash_bytes(&current_bytes);
        let latest_report = gate.latest_report(&full_path).ok().flatten();
        let current_contract_revision = engine
            .session(session_id)
            .and_then(|rec| rec.work_contract.map(|c| c.revision.max(1)));
        let target_revision = latest_report
            .as_ref()
            .and_then(|report| report.contract_revision)
            .or(current_contract_revision);
        if let Some(latest) = &latest_report
            && latest.id == report_id
            && latest.artifact_hash == current_hash
            && let Ok(items) = store.list()
            && let Some(existing) = items.into_iter().find(|item| {
                item.manifest.receipt.session_id == session_id
                    && item.manifest.receipt.delivery_key == delivery_key
                    && item.manifest.receipt.sha256 == current_hash
            })
        {
            let _ = gate.confirm_delivered_by_hash(&current_hash, now);
            if let Some(rev) = target_revision
                && existing
                    .manifest
                    .receipt
                    .submission_id
                    .starts_with("submission-override-")
            {
                let _ = engine.record_submission_revision(
                    session_id,
                    &existing.manifest.receipt.submission_id,
                    rev,
                );
            }
            reconcile_partial_work_after_override(
                &engine,
                &store,
                session_id,
                target_revision,
                Some((
                    existing.manifest.receipt.display_name.as_str(),
                    existing.manifest.receipt.delivery_key.as_str(),
                )),
            );
            let existing_id = existing.manifest.receipt.artifact_id;
            finish_override_live_update(
                state,
                session_id,
                report_id,
                &full_path,
                &existing_id,
                now,
            );
            return Ok(existing_id);
        }
        let stamp = next_stamp();
        let submission_id = format!("submission-override-{stamp:032x}");
        let prepare = crate::artifact_store::PrepareArtifact {
            source_root,
            relative_path: file_name,
            source_relative_path,
            conversation_import: false,
            session_id: session_id.to_owned(),
            submission_id: submission_id.clone(),
            tool_call_id: format!("override-call-{stamp}"),
            delivery_key,
            display_name: None,
            origin: SessionOrigin::Home,
        };
        let result = gate
            .stage_override_for_report_with_revision(
                &full_path,
                report_id,
                current_contract_revision,
                now,
                |checked_bytes| {
                    let expected_hash =
                        crate::delivery::hash_bytes(checked_bytes);
                    let receipt = store.prepare(prepare)?;
                    if receipt.sha256 != expected_hash {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "DELIVERY_ARTIFACT_CHANGED",
                        ));
                    }
                    // Guard against reusing an already-deleted tombstone
                    // manifest from an earlier submission so we never persist
                    // a mismatched DeliveryCommit that poisons session boot.
                    if receipt.submission_id != submission_id
                        && let Ok(Some(existing)) =
                            store.get(&receipt.artifact_id)
                        && matches!(
                            existing.manifest.publication_state,
                            PublicationState::Deleted
                                | PublicationState::DeletePending
                        )
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "DELIVERY_ALREADY_DELETED",
                        ));
                    }
                    Ok(receipt)
                },
            )
            .map_err(|_| "chat.publish.failed")?;
        let receipt = result.value.ok_or("chat.publish.not_verified")?;
        if receipt.submission_id != submission_id {
            // `store.prepare` reused an already-committed snapshot for the
            // exact same `(source_relative_path, sha256)`: do NOT write a
            // conflicting DeliveryCommit with `submission_id` (which
            // `commit_prepared` would reject and poison startup recovery) AND
            // never overwrite an earlier non-override turn's revision mapping.
            let _ = gate.confirm_delivered(
                &result.outcome.delivery_id,
                &full_path.display().to_string(),
                &receipt.sha256,
                now_ms(),
            );
            reconcile_partial_work_after_override(
                &engine,
                &store,
                session_id,
                target_revision,
                Some((
                    receipt.display_name.as_str(),
                    receipt.delivery_key.as_str(),
                )),
            );
            finish_override_live_update(
                state,
                session_id,
                report_id,
                &full_path,
                &receipt.artifact_id,
                now,
            );
            return Ok(receipt.artifact_id);
        }
        let origin_title_snapshot = lock(&state.live)
            .get(session_id)
            .map_or_else(String::new, |live| live.title.clone());
        let commit = DeliveryCommit {
            session_id: session_id.to_owned(),
            submission_id: submission_id.clone(),
            artifact_ids: vec![receipt.artifact_id.clone()],
            completed_successfully: true,
            committed_at_ms: now,
            origin: SessionOrigin::Home,
            origin_title_snapshot,
        };
        if engine.record_delivery_commit(&commit).is_err() {
            let _ = gate.abort_intent(
                &result.outcome.delivery_id,
                &full_path.display().to_string(),
                &receipt.sha256,
                "record_delivery_commit_failed",
                now_ms(),
            );
            return Err("notice.session_storage_failed");
        }
        if let Some(rev) = target_revision {
            let _ = engine.record_submission_revision(
                session_id,
                &submission_id,
                rev,
            );
        }
        if store.commit_prepared(&commit).is_err() {
            // `DeliveryCommit` is already durable in `history.json`: mirror the
            // engine's post-linearization contract by moving the session into
            // `DeliveryFailed` so the user gets an explicit Retry Delivery
            // surface rather than a silent ghost commit on next boot.
            super::mark_delivery_failed(state, session_id);
            if persist_index(state).is_err() {
                state.report_storage_error();
            }
            state.push_update(AgentUpdate::Session {
                id: session_id.to_owned(),
            });
            state.push_update(AgentUpdate::Sessions);
            return Err("chat.status_delivery_failed");
        }
        let _ = gate.confirm_delivered(
            &result.outcome.delivery_id,
            &full_path.display().to_string(),
            &receipt.sha256,
            now_ms(),
        );
        let root_session_id = self.root_session_id_for(session_id);
        tracing::info!(
            target: "flowy.delivery",
            session_id = %session_id,
            root_session_id = %root_session_id,
            submission_id = %submission_id,
            delivery_id = %result.outcome.delivery_id,
            artifact_id = %receipt.artifact_id,
            sha256 = %receipt.sha256,
            override_used = result.outcome.override_used,
            "override delivery committed by user"
        );
        reconcile_partial_work_after_override(
            &engine,
            &store,
            session_id,
            target_revision,
            None,
        );
        finish_override_live_update(
            state,
            session_id,
            report_id,
            &full_path,
            &receipt.artifact_id,
            now,
        );
        Ok(receipt.artifact_id)
    }
}

fn finish_override_live_update(
    state: &UiState,
    session_id: &str,
    report_id: &str,
    full_path: &Path,
    artifact_id: &str,
    now: u64,
) {
    let target_key = crate::delivery::artifact_key(full_path);
    let mut parent_root_to_notify: Option<String> = None;
    if let Some(live) = lock(&state.live).get_mut(session_id) {
        for line in &mut live.projection.lines {
            if let ChatLine::Verification {
                report_id: existing_id,
                artifact_path: existing_path,
                override_used,
                ..
            } = line
                && ((!report_id.is_empty() && existing_id == report_id)
                    || crate::delivery::artifact_key(Path::new(existing_path))
                        == target_key)
            {
                *override_used = true;
            }
        }
        if !live.projection.lines.iter().any(|line| {
            matches!(line, ChatLine::Artifact { artifact_id: id } if id == artifact_id)
        }) {
            live.projection.lines.push(ChatLine::Artifact {
                artifact_id: artifact_id.to_owned(),
            });
        }
        live.updated_at = now;
        if live.role == SessionRole::Worker
            && let Some(root_id) = live.root_session_id.as_deref()
            && root_id != session_id
        {
            parent_root_to_notify = Some(root_id.to_owned());
        }
    }
    if persist_index(state).is_err() {
        state.report_storage_error();
    }
    state.push_update(AgentUpdate::Artifacts);
    state.push_update(AgentUpdate::Session {
        id: session_id.to_owned(),
    });
    if let Some(root_id) = parent_root_to_notify {
        state.push_update(AgentUpdate::Session { id: root_id });
    }
    state.push_update(AgentUpdate::Sessions);
}
