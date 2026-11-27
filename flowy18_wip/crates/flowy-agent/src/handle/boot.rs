//! Agent startup initialization, skill loading, crash recovery, and session restoration.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use flowy_agent_core::{
    AcceptanceQueue, Engine, ReceiptState, SessionRole, TurnObserver,
    TurnState,
    types::{CancelToken, MessageSource, Role},
};
use flowy_skills::{cleanup_staging, load_installed, seed_official};

use super::guards::ensure_tool_cwd;
use super::observer::{
    UiObserver, backfill_conversation_artifacts, confirm_commit_audit,
    finish_interrupted_lines, hydrate_engine, import_engine_sessions,
    mark_delivery_failed, migrate_legacy_source_deletions, persist_index,
    recover_session_deletions, sync_engine_titles,
};
use super::queue::{
    push_reconciled_receipt_updates, reconcile_committed_acceptances,
};
use super::{
    AgentHandle, AgentUpdate, ArtifactStore, ChatLine, ChatProjection, Live,
    LlmConfig, SessionIndex, TurnStatus, engine_config, lock,
    log_storage_io_error, now_ms,
};

impl AgentHandle {
    pub(super) fn initialize(&self) {
        self.reload_skills();
        let artifact_root = self.inner.state.root.join("artifacts");
        match ArtifactStore::open(artifact_root.clone()) {
            Ok(store) => {
                let _ = self.inner.state.artifact_store.set(store);
            }
            Err(error) => {
                log_storage_io_error(
                    "open artifact store",
                    &artifact_root,
                    &error,
                );
            }
        }
        let config = lock(&self.inner.config).clone();
        let sandbox = flowy_agent_core::tools::detect_sandbox_availability();
        tracing::info!(
            target: "flowy.sandbox",
            status = sandbox.telemetry_label(),
            enforced = sandbox.is_enforced(),
            "host sandbox posture probed at boot"
        );
        let boot_receipts = self.boot(config);
        // History is fully loaded and recovered before this point. Keep the
        // library in its existing loading state until the backfill finishes so
        // the UI never flashes an empty catalog first.
        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = &engine {
            backfill_conversation_artifacts(&self.inner.state, engine);
        }
        // A catalog refresh may have arrived while history was being read.
        // Apply its latest route before making initialization observable.
        let cwd = ensure_tool_cwd(&self.inner.state.root);
        {
            let mut initializing = lock(&self.inner.state.initializing);
            let config = lock(&self.inner.config).clone();
            if let Some(engine) = engine {
                engine.set_config(engine_config(&config, &cwd));
            }
            *initializing = false;
        }
        self.inner.state.push_update(AgentUpdate::Initialized);
        self.inner.state.push_update(AgentUpdate::Sessions);
        self.inner.state.push_update(AgentUpdate::Skills);
        self.inner.state.push_update(AgentUpdate::Artifacts);
        push_reconciled_receipt_updates(&self.inner.state, boot_receipts);
        if self.inner.state.autonomous_drain.load(Ordering::Acquire) {
            self.inner.kick_autonomous_drain();
        }
        self.inner.schedule_pending_router_followups();
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::with_config",
            "done"
        );
    }

    fn reload_skills(&self) {
        let root = &self.inner.state.root;
        cleanup_staging(root);
        if let Err(error) = seed_official(root) {
            tracing::warn!(
                target: "flowy.skills",
                error = %error,
                "official skill seed failed"
            );
        }
        match load_installed(root) {
            Ok(catalog) => {
                for failure in &catalog.failures {
                    tracing::warn!(
                        target: "flowy.skills",
                        error = %failure,
                        "installed skill could not be loaded"
                    );
                }
                *lock(&self.inner.skills) = catalog.skills;
            }
            Err(error) => {
                tracing::warn!(
                    target: "flowy.skills",
                    error = %error,
                    "installed skill catalog load failed"
                );
                lock(&self.inner.skills).clear();
            }
        }
    }

    pub(super) fn boot(
        &self,
        config: LlmConfig,
    ) -> Vec<(String, ReceiptState)> {
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::boot",
            "enter"
        );
        let state = Arc::clone(&self.inner.state);
        let index = match SessionIndex::load(&state.root) {
            Ok(index) => index,
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                // Keep the damaged file, but do not let it lock the whole app.
                if let Err(recovery_error) = recover_corrupt_index(&state.root)
                {
                    log_storage_io_error(
                        "quarantine corrupt session index",
                        &SessionIndex::path(&state.root),
                        &recovery_error,
                    );
                    *lock(&state.load_failed) = true;
                    state.report_storage_error();
                    return Vec::new();
                }
                state.report_storage_error();
                SessionIndex::default()
            }
            Err(error) => {
                log_storage_io_error(
                    "open session index",
                    &SessionIndex::path(&state.root),
                    &error,
                );
                *lock(&state.load_failed) = true;
                state.report_storage_error();
                return Vec::new();
            }
        };
        let observer: Arc<dyn TurnObserver> = Arc::new(UiObserver {
            state: Arc::clone(&state),
        });
        let tool_cwd = ensure_tool_cwd(&state.root);
        let engine = match Engine::open(
            state.root.clone(),
            engine_config(&config, &tool_cwd),
            Arc::clone(&observer),
        ) {
            Ok(engine) => Arc::new(engine),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                // A damaged history file must not hide a usable index.
                *lock(&state.history_unavailable) = true;
                if let Err(recovery_error) =
                    recover_corrupt_history(&state.root)
                {
                    log_storage_io_error(
                        "quarantine corrupt structured history",
                        &flowy_agent_core::SessionIndex::path(&state.root),
                        &recovery_error,
                    );
                    *lock(&state.load_failed) = true;
                    state.report_storage_error();
                    return Vec::new();
                }
                state.report_storage_error();
                match Engine::open(
                    state.root.clone(),
                    engine_config(&config, &tool_cwd),
                    observer,
                ) {
                    Ok(engine) => Arc::new(engine),
                    Err(error) => {
                        log_storage_io_error(
                            "reopen structured history after recovery",
                            &flowy_agent_core::SessionIndex::path(&state.root),
                            &error,
                        );
                        *lock(&state.load_failed) = true;
                        state.report_storage_error();
                        return Vec::new();
                    }
                }
            }
            Err(error) => {
                log_storage_io_error(
                    "open structured history",
                    &flowy_agent_core::SessionIndex::path(&state.root),
                    &error,
                );
                *lock(&state.load_failed) = true;
                state.report_storage_error();
                return Vec::new();
            }
        };
        let deleted_sessions = if let Some(store) = state.artifact_store.get() {
            let mut deleted = recover_session_deletions(store, &engine, &state);
            deleted.extend(migrate_legacy_source_deletions(
                store, &engine, &state,
            ));
            deleted
        } else {
            BTreeSet::new()
        };
        let SessionIndex { active, sessions } = index;
        {
            let mut live = lock(&state.live);
            for record in sessions {
                if deleted_sessions.contains(&record.id) {
                    continue;
                }
                let source_deleted =
                    state.artifact_store.get().is_some_and(|store| match store
                        .source_deleted(&record.id)
                    {
                        Ok(deleted) => deleted,
                        Err(error) => {
                            log_storage_io_error(
                                "read source deletion marker",
                                &state.root.join("artifacts"),
                                &error,
                            );
                            false
                        }
                    });
                if engine.session(&record.id).is_none() && source_deleted {
                    continue;
                }
                let mut status = record.status;
                let mut lines = record.lines;
                if status.is_busy() {
                    status = TurnStatus::Stopped;
                    finish_interrupted_lines(&mut lines);
                }
                live.insert(
                    record.id,
                    Live {
                        projection: ChatProjection::from_lines(lines),
                        title: record.title,
                        titled: record.titled,
                        pinned: record.pinned,
                        archived: record.archived,
                        auto_title_pending: false,
                        auto_title_in_flight: false,
                        auto_title_attempts: 0,
                        title_signal: CancelToken::new(),
                        status,
                        updated_at: record.updated_at,
                        turn_started_at: None,
                        workspace: record.workspace,
                        origin: record.origin,
                        session_workspace: record.session_workspace,
                        heartbeat: None,
                        role: record.role,
                        root_session_id: record.root_session_id,
                        access_mode: record.access_mode,
                        active_turn_is_internal: false,
                        active_turn_prev_status: None,
                    },
                );
            }
            drop(live);
            let active_workspace = active.as_ref().and_then(|id| {
                lock(&state.live)
                    .get(id)
                    .and_then(|session| session.workspace.clone())
            });
            *lock(&state.active) = active;
            *lock(&state.workspace_for_new_sessions) = active_workspace;
        }
        // If the UI index was lost or quarantined, rebuild the visible sessions
        // from the structured history instead of hiding them.
        if let Some(store) = state.artifact_store.get() {
            let mut commits = Vec::new();
            let mut recovery_ok = !*lock(&state.history_unavailable);
            for session in engine.sessions() {
                match store.recover_commits(&session.delivery_commits) {
                    Ok(()) => {
                        for commit in &session.delivery_commits {
                            confirm_commit_audit(&state.root, store, commit);
                        }
                        if let Some(live) =
                            lock(&state.live).get_mut(&session.id)
                            && matches!(live.status, TurnStatus::DeliveryFailed)
                        {
                            live.status = TurnStatus::Completed;
                            live.projection.lines.retain(|line| {
                                !matches!(
                                    line,
                                    ChatLine::Status { text }
                                        if text == "chat.status_delivery_failed"
                                )
                            });
                        }
                    }
                    Err(error) => {
                        log_storage_io_error(
                            "recover delivered artifacts",
                            &state.root.join("artifacts"),
                            &error,
                        );
                        recovery_ok = false;
                        mark_delivery_failed(&state, &session.id);
                        state.report_storage_error();
                    }
                }
                commits.extend(session.delivery_commits);
            }
            // This runs before the engine is published to callers, so there
            // are no active runs whose staging files could be removed.
            if let Err(error) = store.cleanup_uncommitted(&commits, recovery_ok)
            {
                log_storage_io_error(
                    "clean up uncommitted artifacts",
                    &state.root.join("artifacts"),
                    &error,
                );
                state.report_storage_error();
            }
        } else {
            tracing::error!(
                target: "flowy.storage",
                operation = "open artifact store",
                root = %state.root.join("artifacts").display(),
                "artifact store unavailable"
            );
            state.report_storage_error();
        }
        import_engine_sessions(&engine, &state, &deleted_sessions);
        // A crash leaves the old status in the file; persist the repaired view.
        let index_persisted = if persist_index(&state) == Ok(()) {
            true
        } else {
            state.report_storage_error();
            false
        };
        if index_persisted && let Some(store) = state.artifact_store.get() {
            for session_id in &deleted_sessions {
                if let Err(error) = store.complete_session_deletion(session_id)
                {
                    log_storage_io_error(
                        "complete recovered session deletion",
                        &state.root.join("artifacts"),
                        &error,
                    );
                    state.report_storage_error();
                }
            }
        }
        hydrate_engine(&engine, &state, &config);
        sync_engine_titles(&engine, &state);
        let mut receipt_updates = reconcile_committed_acceptances(
            &state.root,
            &engine,
            state.artifact_store.get(),
            &[],
        );
        // Desktop is single-instance: any remaining unexpired lease belongs to
        // a crashed predecessor process and must transition immediately to
        // Retrying rather than falsely displaying Running until lease expiry,
        // EXCEPT sessions intentionally parked in WaitingForUser / pending_cards.
        let waiting_sessions: BTreeSet<String> = engine
            .sessions()
            .into_iter()
            .filter(|rec| {
                rec.status == TurnState::WaitingForUser
                    || !rec.pending_cards.is_empty()
            })
            .map(|rec| rec.id)
            .collect();
        if let Ok(released_ids) = AcceptanceQueue::new(state.root.clone())
            .release_uncommitted_leases_except(now_ms(), &waiting_sessions)
        {
            for id in released_ids {
                receipt_updates.push((id, ReceiptState::Retrying));
            }
        }
        // Deterministically clean up any empty Worker left behind by a crash
        // between worker creation and `accept_strict` receipt enqueue, and
        // ensure durable `pending_router_followups` are projected cleanly.
        {
            let all_receipts = AcceptanceQueue::new(state.root.clone())
                .all()
                .unwrap_or_default();
            let mut removed_any = false;
            for rec in engine.sessions() {
                if rec.role == SessionRole::Worker
                    && rec.messages.is_empty()
                    && rec.delivery_commits.is_empty()
                    && rec.pending_cards.is_empty()
                    && rec.work_contract.is_none()
                {
                    let has_live_receipt = all_receipts.iter().any(|r| {
                        r.completed_at_ms.is_none()
                            && !r.cancelled
                            && !r.dead
                            && r.target_session_id
                                .as_deref()
                                .or(r.session_hint.as_deref())
                                == Some(rec.id.as_str())
                    });
                    if !has_live_receipt {
                        let _ = engine.remove_session(&rec.id);
                        if let Some(root_id) = rec.root_session_id.as_deref() {
                            let _ = engine.remove_dispatch_requests_for_worker(
                                root_id, &rec.id,
                            );
                        }
                        lock(&state.live).remove(&rec.id);
                        removed_any = true;
                    }
                } else if rec.role == SessionRole::Router
                    && !rec.pending_router_followups.is_empty()
                    && let Some(live) = lock(&state.live).get_mut(&rec.id)
                {
                    // Reconcile pending follow-ups against both committed
                    // `rec.messages` and already-persisted `live.projection`:
                    // normal restart already has the bubbles in `index.json`,
                    // whereas a crash before `persist_index` (or corrupt index
                    // rebuild) lacks them. Counting surplus projected bubbles
                    // per text preserves two identical follow-ups without
                    // ever duplicating them on clean restart.
                    let mut committed_counts: HashMap<String, usize> =
                        HashMap::new();
                    for msg in &rec.messages {
                        if msg.role == Role::User
                            && !matches!(
                                msg.source,
                                MessageSource::SystemEvent { .. }
                            )
                        {
                            let fallback = msg.text();
                            let label = msg
                                .display_text
                                .as_deref()
                                .unwrap_or(fallback.as_str())
                                .trim();
                            if !label.is_empty() {
                                *committed_counts
                                    .entry(label.to_owned())
                                    .or_insert(0) += 1;
                            }
                        }
                    }
                    let mut projected_counts: HashMap<String, usize> =
                        HashMap::new();
                    for line in &live.projection.lines {
                        if let ChatLine::User { text } = line {
                            *projected_counts
                                .entry(text.trim().to_owned())
                                .or_insert(0) += 1;
                        }
                    }
                    let mut extra_projected: HashMap<String, usize> =
                        HashMap::new();
                    for (text, proj) in projected_counts {
                        let committed =
                            committed_counts.get(&text).copied().unwrap_or(0);
                        if proj > committed {
                            extra_projected.insert(text, proj - committed);
                        }
                    }
                    for item in &rec.pending_router_followups {
                        let already_committed = rec.messages.iter().any(|m| {
                            m.role == Role::User && m.id == item.submission_id
                        });
                        if already_committed {
                            continue;
                        }
                        let key = item.text.trim();
                        if let Some(rem) = extra_projected.get_mut(key)
                            && *rem > 0
                        {
                            *rem -= 1;
                            continue;
                        }
                        live.projection.push_user(&item.text);
                    }
                }
            }
            if removed_any {
                let _ = persist_index(&state);
            }
        }
        if let Ok(unresolved) =
            crate::delivery::DeliveryStore::new(state.root.join("delivery"))
                .in_doubt_deliveries()
        {
            for item in unresolved {
                tracing::warn!(
                    target: "flowy.delivery",
                    delivery_id = %item.delivery_id,
                    artifact_hash = %item.artifact_hash,
                    artifact_path = %item.artifact_path,
                    "unresolved deliver_intent detected at startup"
                );
            }
        }
        *lock(&self.inner.engine) = Some(engine);
        tracing::debug!(
            target: "flowy.boot",
            stage = "AgentHandle::boot",
            sessions = lock(&state.live).len(),
            "done"
        );
        receipt_updates
    }
}

fn recover_corrupt_history(root: &Path) -> std::io::Result<()> {
    let path = flowy_agent_core::SessionIndex::path(root);
    if !path.exists() {
        return Ok(());
    }
    let backup =
        path.with_file_name(format!("history.corrupt-{}.json", now_ms()));
    std::fs::rename(path, backup)
}

fn recover_corrupt_index(root: &Path) -> std::io::Result<()> {
    let path = SessionIndex::path(root);
    if !path.exists() {
        return Ok(());
    }
    let backup =
        path.with_file_name(format!("index.corrupt-{}.json", now_ms()));
    std::fs::rename(path, backup)
}
