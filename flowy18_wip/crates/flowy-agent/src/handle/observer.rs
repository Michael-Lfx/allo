//! `TurnObserver` projection, history hydration, artifact capture, and verification card folding.

use super::{
    AcceptanceQueue, AgentUpdate, AgentWorkspace, Arc, ArtifactStore, BTreeSet,
    BranchHeartbeat, CancelToken, ChatLine, ChatProjection, ContentBlock,
    DeliveryCommit, Engine, FileChangeKind, HashMap, IndexRecord, Live,
    LlmConfig, LoopOutcome, Message, MessageSource, Mutex, Path, PendingCard,
    ReceiptState, Role, SessionDeletionState, SessionIndex, SessionOrigin,
    SessionRecord, SessionRole, StopHandoffManifest, SubmissionOutcome,
    TITLE_MAX_CHARS, TurnObserver, TurnState, TurnStatus, UiState,
    VerificationIssue, complete_text, lock, log_storage_io_error, model_route,
    now_ms, title, tool_target,
};

/// Maximum Unicode scalar characters kept in a sidebar session preview.
pub(super) const PREVIEW_MAX_CHARS: usize = 56;

/// Returns `true` for internal host-generated control/event prompts that must
/// never leak into search results, sidebar previews, or auto-title prompts.
pub(super) fn is_internal_control_line_text(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("[WorkEventBatch]")
        || trimmed.starts_with("[CardChoiceConfirmed]")
        || trimmed.starts_with("[ToolApproved]")
        || trimmed.starts_with("[ToolDenied]")
        || trimmed.starts_with("[UserApproved]")
        || trimmed.starts_with("[UserDenied]")
}

pub(super) fn extract_session_preview(lines: &[ChatLine]) -> Option<String> {
    let raw = lines.iter().rev().find_map(|line| match line {
        ChatLine::Assistant { text } => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        }
        ChatLine::User { text } => {
            let trimmed = text.trim();
            (!trimmed.is_empty() && !is_internal_control_line_text(trimmed))
                .then_some(trimmed)
        }
        _ => None,
    })?;
    let collapsed = title::strip_markdown_inline(raw);
    if collapsed.is_empty() {
        return None;
    }
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(PREVIEW_MAX_CHARS).collect();
    if chars.next().is_some() {
        Some(format!("{head}\u{2026}"))
    } else {
        Some(head)
    }
}

pub(super) struct UiObserver {
    pub(super) state: Arc<UiState>,
}

pub(super) fn mark_live_delivery_failed(session: &mut Live) {
    session.status = TurnStatus::DeliveryFailed;
    session.turn_started_at = None;
    if !session.projection.lines.iter().any(|line| {
        matches!(
            line,
            ChatLine::Status { text }
                if text == "chat.status_delivery_failed"
        )
    }) {
        session.projection.lines.push(ChatLine::Status {
            text: "chat.status_delivery_failed".into(),
        });
    }
}

pub(super) fn mark_delivery_failed(state: &UiState, session_id: &str) {
    if let Some(session) = lock(&state.live).get_mut(session_id) {
        mark_live_delivery_failed(session);
    }
    state.stream_changed(session_id);
}

pub(super) fn apply_delivery_commit_to_live(
    state: &UiState,
    commit: &DeliveryCommit,
) {
    if let Some(session) = lock(&state.live).get_mut(&commit.session_id) {
        session.projection.flush();
        for artifact_id in &commit.artifact_ids {
            if !session.projection.lines.iter().any(|line| {
                matches!(
                    line,
                    ChatLine::Artifact { artifact_id: existing }
                        if existing == artifact_id
                )
            }) {
                session.projection.lines.push(ChatLine::Artifact {
                    artifact_id: artifact_id.clone(),
                });
            }
        }
    }
}

impl TurnObserver for UiObserver {
    fn saving_files(&self, session_id: &str) {
        if let Some(session) = lock(&self.state.live).get_mut(session_id) {
            session.status = TurnStatus::SavingFiles;
            let hb = session
                .heartbeat
                .get_or_insert_with(BranchHeartbeat::default);
            "saving_files".clone_into(&mut hb.current_action);
            hb.next_check_hint = Some("committing delivery markers".to_owned());
            hb.awaiting_evidence = false;
        }
        self.state.stream_changed(session_id);
    }

    fn delivery_committed(&self, commit: &DeliveryCommit) {
        if let Some(store) = self.state.artifact_store.get() {
            confirm_commit_audit(&self.state.root, store, commit);
        }
        apply_delivery_commit_to_live(&self.state, commit);
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
        self.state.stream_changed(&commit.session_id);
        self.notify_root_if_worker(&commit.session_id);
        self.state.push_update(AgentUpdate::Artifacts);
    }

    fn delivery_failed(&self, commit: &DeliveryCommit, error: &str) {
        tracing::error!(
            target: "flowy.storage",
            operation = "commit delivered artifacts",
            session = %commit.session_id,
            submission = %commit.submission_id,
            error = %error,
            "delivery marker failed"
        );
        if let Some(session) =
            lock(&self.state.live).get_mut(&commit.session_id)
        {
            session.status = TurnStatus::DeliveryFailed;
            session.turn_started_at = None;
        }
        self.state.stream_changed(&commit.session_id);
        self.notify_root_if_worker(&commit.session_id);
    }

    fn text(&self, session: &str, delta: &str) {
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.apply_text(delta);
        }
        self.state.stream_changed(session);
    }

    fn reasoning(&self, session: &str, delta: &str) {
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.apply_reasoning(delta);
            let hb =
                live.heartbeat.get_or_insert_with(BranchHeartbeat::default);
            if hb.current_action.is_empty() || hb.current_action == "thinking" {
                "reasoning".clone_into(&mut hb.current_action);
                hb.next_check_hint =
                    Some("awaiting next tool call or response".to_owned());
                hb.awaiting_evidence = true;
            }
        }
        self.state.stream_changed(session);
    }

    fn assistant_text(&self, session: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection.flush();
            let duplicate = matches!(
                live.projection.lines.last(),
                Some(ChatLine::Assistant { text: last }) if last == text
            );
            if !duplicate {
                live.projection.lines.push(ChatLine::Assistant {
                    text: text.to_owned(),
                });
            }
        }
    }

    fn tool_start(
        &self,
        session: &str,
        call_id: &str,
        name: &str,
        args: &serde_json::Value,
    ) {
        // Only file-writing tools can produce a change row; keep their
        // arguments until the call settles.
        if matches!(name, "write_file" | "edit_file") {
            lock(&self.state.tool_args)
                .insert(tool_arg_key(session, call_id), args.clone());
        }
        let target = tool_target(args);
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection
                .push_tool_call(name, target.clone(), call_id);
            let hb =
                live.heartbeat.get_or_insert_with(BranchHeartbeat::default);
            hb.current_action = if target.is_empty() {
                name.to_owned()
            } else {
                format!(
                    "{name}: {}",
                    flowy_agent_core::log_preview(&target, 80)
                )
            };
            hb.next_check_hint = Some(format!("awaiting result of {name}"));
            hb.awaiting_evidence = true;
            live.updated_at = now_ms();
        }
        self.state.stream_changed(session);
    }

    fn tool_end(
        &self,
        session: &str,
        call_id: &str,
        name: &str,
        text: &str,
        is_error: bool,
    ) {
        // Tool paths may be relative; resolve them against the session working
        // directory so the card can open the real folder later.
        let cwd = lock(&self.state.live)
            .get(session)
            .and_then(|live| {
                live.session_workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone())
                    .or_else(|| {
                        live.workspace
                            .as_ref()
                            .map(|workspace| workspace.root.clone())
                    })
            })
            .unwrap_or_else(|| self.state.root.join("sessions").join("cwd"));
        let display_text = tool_result_display_text(name, text);
        let verification_line =
            (matches!(name, "verify_deck" | "verify_artifact") && !is_error)
                .then(|| {
                    let gate = crate::delivery::DeliveryStore::new(
                        self.state.root.join("delivery"),
                    );
                    parse_verification_line(text, Some(&gate))
                })
                .flatten();
        if let Some(live) = lock(&self.state.live).get_mut(session) {
            live.projection
                .finish_tool_call(call_id, &display_text, is_error);
            if let Some(card) = verification_line {
                upsert_verification_line(&mut live.projection.lines, card);
            }
            // Drop the cached arguments either way; a failed call leaves the
            // file untouched, so only settled calls get a change row.
            let change =
                take_file_change(&self.state.tool_args, session, call_id, &cwd);
            if !is_error && let Some(change) = change {
                live.projection.push_file_change(
                    change.path,
                    change.kind,
                    change.added,
                    change.removed,
                    change.binary,
                );
            }
            let now = now_ms();
            let hb =
                live.heartbeat.get_or_insert_with(BranchHeartbeat::default);
            hb.last_completed = Some(if is_error {
                format!("{name} (error)")
            } else {
                name.to_owned()
            });
            hb.last_completed_at_ms = Some(now);
            "evaluating tool evidence".clone_into(&mut hb.current_action);
            hb.next_check_hint = Some("awaiting next model step".to_owned());
            hb.awaiting_evidence = false;
            live.updated_at = now;
        }
        self.state.stream_changed(session);
    }

    fn finished(&self, session: &str, outcome: LoopOutcome) {
        let next_status = {
            let mut live = lock(&self.state.live);
            live.get_mut(session).map(|live| {
                live.projection.flush();
                promote_latest_turn_verifications(&mut live.projection.lines);
                let next_status = match outcome {
                    LoopOutcome::Completed => TurnStatus::Completed,
                    LoopOutcome::Aborted => TurnStatus::Stopped,
                    LoopOutcome::WaitingForUser => TurnStatus::WaitingForUser,
                    // `delivery_failed` ran before this terminal callback. Keep
                    // the dedicated status so the retry action remains visible.
                    LoopOutcome::Failed
                        if matches!(
                            live.status,
                            TurnStatus::DeliveryFailed
                        ) =>
                    {
                        TurnStatus::DeliveryFailed
                    }
                    // Store an i18n key, never a raw English message: this value
                    // is persisted in the index and may be rendered later.
                    LoopOutcome::Failed => TurnStatus::Error {
                        message: "chat.status_error".into(),
                    },
                };
                if outcome != LoopOutcome::Completed {
                    live.projection.lines.push(ChatLine::Status {
                        text: next_status.key().into(),
                    });
                }
                clear_live_heartbeat(live);
                live.turn_started_at = None;
                live.updated_at = now_ms();
                next_status
            })
        };
        self.finish_session_settle(session, next_status, true);
    }

    fn waiting_for_user(&self, session: &str, card: &PendingCard) {
        let hint = match card {
            PendingCard::ToolApproval { tool_name, .. } => {
                format!("awaiting user approval for {tool_name}")
            }
            PendingCard::Disambiguation { .. } => {
                "awaiting user disambiguation selection".to_owned()
            }
        };
        let has_live = {
            let mut live = lock(&self.state.live);
            if let Some(live) = live.get_mut(session) {
                live.projection.flush();
                let already_waiting_line = matches!(
                    live.projection.lines.last(),
                    Some(ChatLine::Status { text })
                        if text == TurnStatus::WaitingForUser.key()
                );
                if !already_waiting_line {
                    live.projection.lines.push(ChatLine::Status {
                        text: TurnStatus::WaitingForUser.key().into(),
                    });
                }
                let hb =
                    live.heartbeat.get_or_insert_with(BranchHeartbeat::default);
                "waiting_for_user".clone_into(&mut hb.current_action);
                hb.next_check_hint = Some(hint);
                hb.awaiting_evidence = true;
                live.turn_started_at = None;
                live.updated_at = now_ms();
                true
            } else {
                false
            }
        };
        self.finish_session_settle(
            session,
            has_live.then_some(TurnStatus::WaitingForUser),
            false,
        );
    }

    fn storage_error(&self, session: &str) {
        tracing::error!(
            target: "flowy.storage",
            operation = "persist structured session history",
            session = %session,
            path = %flowy_agent_core::SessionIndex::path(&self.state.root).display(),
            "agent reported a storage failure"
        );
        self.state.report_storage_error();
    }

    fn stop_settled(&self, session: &str, _manifest: &StopHandoffManifest) {
        let should_mark_stopped = {
            let mut live = lock(&self.state.live);
            live.get_mut(session).is_some_and(|live| {
                live.projection.flush();
                let transition = matches!(
                    live.status,
                    TurnStatus::Running
                        | TurnStatus::Stopping
                        | TurnStatus::SavingFiles
                );
                if transition {
                    let already_stopped_line = matches!(
                        live.projection.lines.last(),
                        Some(ChatLine::Status { text })
                            if text == TurnStatus::Stopped.key()
                    );
                    if !already_stopped_line {
                        live.projection.lines.push(ChatLine::Status {
                            text: TurnStatus::Stopped.key().into(),
                        });
                    }
                }
                clear_live_heartbeat(live);
                live.turn_started_at = None;
                live.updated_at = now_ms();
                transition
            })
        };
        self.finish_session_settle(
            session,
            should_mark_stopped.then_some(TurnStatus::Stopped),
            true,
        );
    }
}

pub(super) fn clear_live_heartbeat(live: &mut Live) {
    if let Some(hb) = live.heartbeat.as_mut() {
        hb.current_action.clear();
        hb.next_check_hint = None;
        hb.awaiting_evidence = false;
    }
}

impl UiObserver {
    fn notify_root_if_worker(&self, session: &str) {
        let root_id = lock(&self.state.live).get(session).and_then(|live| {
            (live.role == SessionRole::Worker)
                .then(|| live.root_session_id.clone())
                .flatten()
        });
        if let Some(root_id) = root_id
            && root_id != session
        {
            self.state.push_update(AgentUpdate::Session { id: root_id });
        }
    }

    fn finish_session_settle(
        &self,
        session: &str,
        staged_status: Option<TurnStatus>,
        clear_denials: bool,
    ) {
        if clear_denials {
            lock(&self.state.denied_tool_turns).remove(session);
        }
        lock(&self.state.approved_tool_calls).remove(session);
        lock(&self.state.tool_args)
            .retain(|key, _| !key.starts_with(&format!("{session}\u{1}")));
        // Persist index.json with the target status BEFORE flipping `live.status`
        // out of `Running`, so callers polling `session_status` never observe a
        // terminal status while disk I/O still holds the `RunFence` ticket.
        if persist_index_with_status(
            &self.state,
            staged_status.as_ref().map(|status| (session, status)),
        )
        .is_err()
        {
            self.state.report_storage_error();
        }
        if let Some(status) = staged_status
            && let Some(live) = lock(&self.state.live).get_mut(session)
        {
            live.status = status;
        }
        self.state.push_update(AgentUpdate::Session {
            id: session.to_owned(),
        });
        self.notify_root_if_worker(session);
        self.state.push_update(AgentUpdate::Sessions);
    }
}

pub(super) async fn generate_title(
    engine: Arc<Engine>,
    state: Arc<UiState>,
    config: LlmConfig,
    signal: CancelToken,
    id: String,
    prompt: String,
) {
    let acquired = {
        let mut live = lock(&state.live);
        live.get_mut(&id).is_some_and(|session| {
            if session.role == SessionRole::Worker
                || !session.auto_title_pending
                || session.auto_title_in_flight
                || session.auto_title_attempts >= super::MAX_AUTO_TITLE_ATTEMPTS
                || signal.is_cancelled()
            {
                return false;
            }
            session.auto_title_in_flight = true;
            session.auto_title_attempts =
                session.auto_title_attempts.saturating_add(1);
            true
        })
    };
    if !acquired {
        return;
    }
    let prompt: String = prompt.chars().take(2000).collect();
    let generated = complete_text(
        Some(format!("title-{id}")),
        title::title_system_prompt(),
        &prompt,
        model_route(&config),
        config.api_key.clone(),
        signal.clone(),
    )
    .await;
    let title = if signal.is_cancelled() {
        None
    } else {
        generated
            .ok()
            .map(|text| title::clamp_title(&text, TITLE_MAX_CHARS))
            .filter(|text| !text.is_empty())
    };
    let Some(title) = title else {
        if let Some(session) = lock(&state.live).get_mut(&id) {
            session.auto_title_in_flight = false;
            if session.auto_title_attempts >= super::MAX_AUTO_TITLE_ATTEMPTS {
                session.auto_title_pending = false;
            }
        }
        return;
    };
    // Serialize the title commit with session deletion and manual rename.
    // Once the user renames or deletes the session, this task must not overwrite it.
    let _dispatch = lock(&state.dispatch);
    let still_pending = lock(&state.live)
        .get(&id)
        .is_some_and(|session| session.auto_title_pending);
    if signal.is_cancelled() || !still_pending {
        if let Some(session) = lock(&state.live).get_mut(&id) {
            session.auto_title_in_flight = false;
        }
        return;
    }
    if engine.set_session_title(&id, title.clone(), true).is_err() {
        if let Some(session) = lock(&state.live).get_mut(&id) {
            session.auto_title_in_flight = false;
        }
        state.report_storage_error();
        return;
    }
    sync_artifact_title(&state, &engine, &id);
    if let Some(session) = lock(&state.live).get_mut(&id) {
        session.title = title;
        session.titled = true;
        session.auto_title_pending = false;
        session.auto_title_in_flight = false;
        session.updated_at = now_ms();
    }
    if persist_index(&state).is_err() {
        state.report_storage_error();
    }
    state.push_update(AgentUpdate::Session { id });
    state.push_update(AgentUpdate::Sessions);
}

pub(super) fn recover_session_deletions(
    store: &ArtifactStore,
    engine: &Engine,
    state: &UiState,
) -> BTreeSet<String> {
    let mut deleted = BTreeSet::new();
    let plans = match store.session_deletions() {
        Ok(plans) => plans,
        Err(error) => {
            log_storage_io_error(
                "load session deletion plans",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
            return deleted;
        }
    };
    for plan in plans {
        let session_exists = engine.session(&plan.session_id).is_some();
        let any_child_missing = plan
            .child_session_ids
            .iter()
            .any(|cid| engine.session(cid).is_none());
        let roll_forward = |deleted: &mut BTreeSet<String>| {
            deleted.insert(plan.session_id.clone());
            for cid in &plan.child_session_ids {
                deleted.insert(cid.clone());
            }
            cancel_receipts_for_sessions(
                state,
                std::iter::once(plan.session_id.as_str())
                    .chain(plan.child_session_ids.iter().map(String::as_str)),
            );
            for cid in &plan.child_session_ids {
                if engine.session(cid).is_some()
                    && let Err(error) = engine.remove_session(cid)
                {
                    log_storage_io_error(
                        "remove child session for cascade deletion",
                        &flowy_agent_core::SessionIndex::path(&state.root),
                        &error,
                    );
                    state.report_storage_error();
                }
            }
            if engine.session(&plan.session_id).is_some()
                && let Err(error) = engine.remove_session(&plan.session_id)
            {
                log_storage_io_error(
                    "remove session for committed deletion",
                    &flowy_agent_core::SessionIndex::path(&state.root),
                    &error,
                );
                state.report_storage_error();
            }
        };
        let result =
            match plan.state {
                SessionDeletionState::Prepared
                    if session_exists && !any_child_missing =>
                {
                    store.cancel_session_deletion(&plan.session_id)
                }
                SessionDeletionState::Prepared => {
                    roll_forward(&mut deleted);
                    store.commit_session_deletion(&plan.session_id).and_then(
                        |()| store.cleanup_session_deletion(&plan.session_id),
                    )
                }
                SessionDeletionState::Committed => {
                    roll_forward(&mut deleted);
                    store.cleanup_session_deletion(&plan.session_id)
                }
            };
        if let Err(error) = result {
            log_storage_io_error(
                "recover session deletion",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
        }
    }
    deleted
}

pub(super) fn cancel_receipts_for_sessions<'a>(
    state: &UiState,
    session_ids: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let targets: BTreeSet<&str> = session_ids.into_iter().collect();
    if targets.is_empty() {
        return Vec::new();
    }
    let queue = AcceptanceQueue::new(state.root.clone());
    let Ok(receipts) = queue.all() else {
        return Vec::new();
    };
    let now = now_ms();
    let mut cancelled = Vec::new();
    for receipt in receipts {
        if receipt.completed_at_ms.is_some()
            || receipt.dead
            || receipt.cancelled
        {
            continue;
        }
        let matches = receipt
            .target_session_id
            .as_deref()
            .is_some_and(|sid| targets.contains(sid))
            || receipt
                .session_hint
                .as_deref()
                .is_some_and(|sid| targets.contains(sid));
        if matches && queue.cancel(&receipt.id, now).unwrap_or(false) {
            state.push_update(AgentUpdate::Receipt {
                id: receipt.id.clone(),
                state: ReceiptState::Cancelled,
            });
            cancelled.push(receipt.id);
        }
    }
    cancelled
}

pub(super) fn migrate_legacy_source_deletions(
    store: &ArtifactStore,
    engine: &Engine,
    state: &UiState,
) -> BTreeSet<String> {
    let mut deleted = BTreeSet::new();
    let sessions = match store.source_deleted_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            log_storage_io_error(
                "load legacy source deletion markers",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
            return deleted;
        }
    };
    for session_id in sessions {
        if engine.session(&session_id).is_some() {
            if let Err(error) = store.clear_source_deleted(&session_id) {
                log_storage_io_error(
                    "clear stale source deletion marker",
                    &state.root.join("artifacts"),
                    &error,
                );
                state.report_storage_error();
            }
            continue;
        }
        deleted.insert(session_id.clone());
        let result = store
            .prepare_session_deletion(&session_id, &[], &[])
            .and_then(|()| store.commit_session_deletion(&session_id))
            .and_then(|()| store.cleanup_session_deletion(&session_id));
        if let Err(error) = result {
            log_storage_io_error(
                "migrate legacy source deletion",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
        }
    }
    deleted
}

pub(super) fn backfill_conversation_artifacts(
    state: &UiState,
    engine: &Engine,
) {
    let Some(store) = state.artifact_store.get().cloned() else {
        return;
    };
    for session in engine.sessions() {
        if !crate::conversation_artifacts::session_supports_import(&session) {
            continue;
        }
        match store.source_deleted(&session.id) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                log_storage_io_error(
                    "read source deletion marker before artifact backfill",
                    &state.root.join("artifacts"),
                    &error,
                );
                state.report_storage_error();
                continue;
            }
        }
        let writes: Vec<_> =
            crate::conversation_artifacts::parse_successful_writes(
                &session.messages,
            )
            .into_iter()
            .filter(|write| {
                !matches!(
                    session.submission_outcomes.get(&write.turn_id),
                    Some(
                        SubmissionOutcome::Failed { .. }
                            | SubmissionOutcome::Aborted
                    )
                )
            })
            .collect();
        if writes.is_empty() {
            continue;
        }
        let contract_revision = session
            .work_contract
            .as_ref()
            .map(|contract| contract.revision.max(1));
        let gate =
            crate::delivery::DeliveryStore::new(state.root.join("delivery"));
        let commits =
            crate::conversation_artifacts::prepare_conversation_artifacts_gated(
                &store,
                Some(&gate),
                &session,
                &writes,
            );
        persist_conversation_commits(
            engine,
            &store,
            state,
            commits,
            contract_revision,
        );
    }
}

pub(super) fn capture_turn_artifacts(
    engine: &Engine,
    state: &UiState,
    session_id: &str,
    submission: Option<u128>,
) {
    let Some(submission) = submission else {
        return;
    };
    let Some(session) = engine.session(session_id) else {
        return;
    };
    let turn_id =
        crate::conversation_artifacts::turn_id_for_submission(submission);
    if !session.messages.iter().any(|message| message.id == turn_id) {
        tracing::warn!(
            target: "flowy.storage",
            session = %session_id,
            turn = %turn_id,
            "conversation artifact import could not locate the finished turn"
        );
        return;
    }
    let writes =
        crate::conversation_artifacts::parse_successful_writes_for_turn(
            &session.messages,
            &turn_id,
        );
    if writes.is_empty() {
        return;
    }
    let Some(store) = state.artifact_store.get().cloned() else {
        state.report_storage_error();
        return;
    };
    let turn_revision = session.submission_revisions.get(&turn_id).copied();
    let gate = crate::delivery::DeliveryStore::new(state.root.join("delivery"));
    let commits =
        crate::conversation_artifacts::prepare_conversation_artifacts_gated(
            &store,
            Some(&gate),
            &session,
            &writes,
        );
    if persist_conversation_commits(
        engine,
        &store,
        state,
        commits,
        turn_revision,
    ) {
        super::work::reconcile_partial_work_for_revision(
            engine,
            &store,
            session_id,
            turn_revision,
        );
        state.push_update(AgentUpdate::Artifacts);
    }
}

pub(super) fn persist_conversation_commits(
    engine: &Engine,
    store: &ArtifactStore,
    state: &UiState,
    commits: Vec<DeliveryCommit>,
    contract_revision: Option<u32>,
) -> bool {
    let mut imported = false;
    for commit in commits {
        if let Err(error) = engine.record_delivery_commit(&commit) {
            log_storage_io_error(
                "persist conversation artifact commit",
                &flowy_agent_core::SessionIndex::path(&state.root),
                &error,
            );
            store.record_import_diagnostic(format!(
                "Could not persist artifact commit for {}: {error}",
                commit.session_id
            ));
            state.report_storage_error();
            continue;
        }
        if let Some(revision) = contract_revision {
            let _ = engine.record_submission_revision(
                &commit.session_id,
                &commit.submission_id,
                revision,
            );
        }
        if let Err(error) = store.commit_prepared(&commit) {
            log_storage_io_error(
                "commit conversation artifact",
                &state.root.join("artifacts"),
                &error,
            );
            store.record_import_diagnostic(format!(
                "Could not commit conversation artifact for {}: {error}",
                commit.session_id
            ));
            state.report_storage_error();
            continue;
        }
        confirm_commit_audit(&state.root, store, &commit);
        imported = true;
    }
    imported
}

/// Confirm `deliver` audit events strictly after `commit`'s Library delivery
/// marker exists on disk. Idempotent across startup recovery and retries.
pub(super) fn confirm_commit_audit(
    root: &Path,
    store: &ArtifactStore,
    commit: &DeliveryCommit,
) {
    if !commit.completed_successfully {
        return;
    }
    let gate = crate::delivery::DeliveryStore::new(root.join("delivery"));
    let now = now_ms();
    for artifact_id in &commit.artifact_ids {
        if let Ok(Some(record)) = store.get(artifact_id) {
            let _ = gate.confirm_delivered_by_hash(
                &record.manifest.receipt.sha256,
                now,
            );
        }
    }
}

pub(super) fn sync_artifact_title(state: &UiState, engine: &Engine, id: &str) {
    if let Some(store) = state.artifact_store.get() {
        let commits = engine
            .session(id)
            .map_or_default(|record| record.delivery_commits);
        if let Err(error) = store.recover_commits(&commits) {
            log_storage_io_error(
                "update artifact source title",
                &state.root.join("artifacts"),
                &error,
            );
            state.report_storage_error();
        }
    }
}

pub(super) fn persist_index(state: &UiState) -> Result<(), &'static str> {
    persist_index_with_status(state, None)
}

fn persist_index_with_status(
    state: &UiState,
    status_override: Option<(&str, &TurnStatus)>,
) -> Result<(), &'static str> {
    // Serialize snapshots and replacements without holding the read model
    // mutex while the filesystem syncs, so UI reads remain memory-only.
    let _write = lock(&state.index_write);
    let live = lock(&state.live);
    let mut sessions: Vec<IndexRecord> = live
        .iter()
        .map(|(id, session)| IndexRecord {
            id: id.clone(),
            title: session.title.clone(),
            updated_at: session.updated_at,
            lines: session.projection.lines.clone(),
            status: match status_override {
                Some((target_id, status)) if id == target_id => status.clone(),
                _ => session.status.clone(),
            },
            titled: session.titled,
            workspace: session.workspace.clone(),
            origin: session.origin.clone(),
            session_workspace: session.session_workspace.clone(),
            pinned: session.pinned,
            archived: session.archived,
            role: session.role,
            root_session_id: session.root_session_id.clone(),
            access_mode: session.access_mode,
        })
        .collect();
    sessions.sort_by_key(|row| std::cmp::Reverse(row.updated_at));
    drop(live);
    let active = lock(&state.active).clone();
    let index = SessionIndex { active, sessions };
    index
        .save(&state.root)
        .map_err(|_| "notice.session_storage_failed")
}

pub(super) fn finish_interrupted_lines(lines: &mut [ChatLine]) {
    for line in lines {
        match line {
            ChatLine::Reasoning { live, .. } => *live = false,
            ChatLine::ToolCall { running, .. } => *running = false,
            _ => {}
        }
    }
}

pub(super) fn ensure_engine_session(
    engine: &Engine,
    id: &str,
) -> std::io::Result<()> {
    if engine.session(id).is_none() {
        engine.insert_session(SessionRecord {
            id: id.to_owned(),
            ..SessionRecord::default()
        })?;
    }
    Ok(())
}

pub(super) fn import_engine_sessions(
    engine: &Engine,
    state: &UiState,
    deleted_sessions: &BTreeSet<String>,
) {
    let records = engine.sessions();
    let committed = state
        .artifact_store
        .get()
        .and_then(|store| store.committed_ids().ok())
        .unwrap_or_default();
    let delivery_gate =
        crate::delivery::DeliveryStore::new(state.root.join("delivery"));
    let override_index = delivery_gate.override_index().ok();
    let mut live = lock(&state.live);
    for record in records {
        if deleted_sessions.contains(&record.id) {
            continue;
        }
        let allowed = record
            .delivery_commits
            .iter()
            .flat_map(|commit| &commit.artifact_ids)
            .filter(|id| committed.contains(*id))
            .cloned()
            .collect::<BTreeSet<_>>();
        let override_artifacts: Vec<String> = record
            .delivery_commits
            .iter()
            .filter(|commit| {
                commit.completed_successfully
                    && commit.submission_id.starts_with("submission-override-")
            })
            .flat_map(|commit| &commit.artifact_ids)
            .filter(|id| allowed.contains(*id))
            .cloned()
            .collect();
        let pending_delivery = record
            .delivery_commits
            .iter()
            .filter(|commit| commit.completed_successfully)
            .flat_map(|commit| &commit.artifact_ids)
            .any(|id| !committed.contains(id));
        if let Some(existing) = live.get_mut(&record.id) {
            existing.origin = record.origin.clone();
            existing.session_workspace.clone_from(&record.workspace);
            existing.pinned = record.pinned;
            existing.archived = record.archived;
            existing.role = record.role;
            existing.root_session_id.clone_from(&record.root_session_id);
            existing.access_mode = record.access_mode;
            let existing_ids = existing
                .projection
                .lines
                .iter()
                .filter_map(|line| {
                    if let ChatLine::Artifact { artifact_id } = line {
                        Some(artifact_id.clone())
                    } else {
                        None
                    }
                })
                .collect::<BTreeSet<_>>();
            if existing_ids != allowed {
                let mut projected = lines_from_messages_with_index(
                    &record.messages,
                    &allowed,
                    override_index.as_ref(),
                );
                append_missing_artifacts(&mut projected, &override_artifacts);
                projected.extend(
                    existing
                        .projection
                        .lines
                        .iter()
                        .filter(|line| {
                            matches!(line, ChatLine::TaskReceipt { .. })
                        })
                        .cloned(),
                );
                existing.projection = ChatProjection::from_lines(projected);
                existing.status = live_status(record.status.clone());
                existing.updated_at = record.updated_at;
            }
            let waiting_for_user = record.status == TurnState::WaitingForUser
                || !record.pending_cards.is_empty();
            restore_existing_history(
                existing,
                record,
                &allowed,
                override_index.as_ref(),
            );
            if let Some(index) = &override_index {
                sync_verification_overrides_with_index(
                    &mut existing.projection.lines,
                    index,
                );
            }
            append_missing_artifacts(
                &mut existing.projection.lines,
                &override_artifacts,
            );
            if pending_delivery {
                mark_live_delivery_failed(existing);
            } else if waiting_for_user {
                existing.status = TurnStatus::WaitingForUser;
            }
            continue;
        }
        let mut lines = lines_from_messages_with_index(
            &record.messages,
            &allowed,
            override_index.as_ref(),
        );
        append_missing_artifacts(&mut lines, &override_artifacts);
        let title = if record.title.is_empty() {
            lines
                .iter()
                .find_map(|line| match line {
                    ChatLine::User { text }
                        if !is_internal_control_line_text(text) =>
                    {
                        Some(title::fallback_title(text, TITLE_MAX_CHARS))
                    }
                    _ => None,
                })
                .unwrap_or_default()
        } else {
            record.title.clone()
        };
        let titled = record.titled || !title.is_empty();
        let status = if pending_delivery {
            TurnStatus::DeliveryFailed
        } else {
            live_status(record.status)
        };
        live.insert(
            record.id.clone(),
            Live {
                projection: ChatProjection::from_lines(lines),
                title,
                titled,
                pinned: record.pinned,
                archived: record.archived,
                auto_title_pending: false,
                auto_title_in_flight: false,
                auto_title_attempts: 0,
                title_signal: CancelToken::new(),
                status,
                updated_at: record.updated_at,
                turn_started_at: None,
                workspace: match &record.origin {
                    SessionOrigin::Project {
                        project_id: Some(project_id),
                    } => record.workspace.as_ref().map(|workspace| {
                        AgentWorkspace {
                            project_id: project_id.clone(),
                            root: workspace.root.clone(),
                        }
                    }),
                    _ => None,
                },
                origin: record.origin,
                session_workspace: record.workspace,
                heartbeat: None,
                role: record.role,
                root_session_id: record.root_session_id,
                access_mode: record.access_mode,
            },
        );
    }
}

fn is_internal_resume_message(message: &Message) -> bool {
    message.role == Role::User
        && (matches!(
            message.source,
            MessageSource::CardResume | MessageSource::SystemEvent { .. }
        ) || (matches!(message.source, MessageSource::User)
            && message.display_text.as_deref() == Some("")))
}

pub(super) fn restore_existing_history(
    existing: &mut Live,
    record: SessionRecord,
    allowed: &BTreeSet<String>,
    overrides: Option<&crate::delivery::OverrideIndex>,
) {
    let lines = &mut existing.projection.lines;
    let mut visible = lines.iter().enumerate().filter_map(|(index, line)| {
        if let ChatLine::User { text } = line {
            Some((index, text))
        } else {
            None
        }
    });
    let mut latest = None;
    for (index, message) in
        record.messages.iter().enumerate().filter(|(_, message)| {
            message.role == Role::User
                && matches!(message.source, MessageSource::User)
                && !is_internal_resume_message(message)
        })
    {
        let display = message
            .display_text
            .clone()
            .unwrap_or_else(|| message.text());
        match visible.next() {
            Some((row, text)) if *text == display => {
                latest = Some((row, index));
            }
            None if message.id.starts_with("submission-") => {
                lines.extend(lines_from_messages_with_index(
                    &record.messages[index..],
                    allowed,
                    overrides,
                ));
                existing.status = live_status(record.status);
                existing.updated_at = record.updated_at;
                return;
            }
            _ => return,
        }
    }
    // The completed turn can reach disk before the UI index. Match both
    // prompt sequences so an older reply cannot complete a later rerun.
    let Some((row, index)) = latest else {
        return;
    };
    if visible.next().is_some()
        || !matches!(
            existing.status,
            TurnStatus::Stopped | TurnStatus::WaitingForUser
        )
        || record.status != TurnState::Completed
        || record.updated_at < existing.updated_at
    {
        return;
    }

    let previous = lines.split_off(row + 1);
    let mut timings = previous
        .iter()
        .filter_map(|line| match line {
            ChatLine::Reasoning {
                text,
                duration_secs,
                ..
            } => Some((text, *duration_secs)),
            _ => None,
        })
        .peekable();
    let mut recovered = lines_from_messages_with_index(
        &record.messages[index + 1..],
        allowed,
        overrides,
    );
    for line in &mut recovered {
        if let ChatLine::Reasoning {
            text,
            duration_secs,
            ..
        } = line
            && let Some((_, elapsed)) =
                timings.next_if(|(saved, _)| *saved == text)
        {
            *duration_secs = elapsed;
        }
    }
    lines.extend(recovered);
    // Task receipts belong to the UI and have no structured message.
    lines.extend(
        previous
            .into_iter()
            .filter(|line| matches!(line, ChatLine::TaskReceipt { .. })),
    );
    existing.status = TurnStatus::Completed;
    existing.updated_at = record.updated_at;
}

pub(super) fn append_missing_artifacts(
    lines: &mut Vec<ChatLine>,
    ids: &[String],
) {
    for id in ids {
        if !lines.iter().any(|line| {
            matches!(line, ChatLine::Artifact { artifact_id } if artifact_id == id)
        }) {
            lines.push(ChatLine::Artifact {
                artifact_id: id.clone(),
            });
        }
    }
}

pub(super) fn sync_verification_overrides_with_index(
    lines: &mut [ChatLine],
    overrides: &crate::delivery::OverrideIndex,
) {
    for line in lines {
        if let ChatLine::Verification {
            report_id,
            artifact_path,
            artifact_hash,
            override_used,
            ..
        } = line
            && !*override_used
            && (!report_id.is_empty() || !artifact_hash.is_empty())
            && overrides.report_was_overridden(
                report_id,
                artifact_path,
                artifact_hash,
            )
        {
            *override_used = true;
        }
    }
}

/// Keep only the latest verification conclusion card per normalized artifact
/// path so a fixed & passing re-verification supersedes earlier failed cards.
pub(super) fn upsert_verification_line(
    lines: &mut Vec<ChatLine>,
    card: ChatLine,
) {
    if let ChatLine::Verification { artifact_path, .. } = &card {
        let key = crate::delivery::artifact_key(Path::new(artifact_path));
        lines.retain(|existing| match existing {
            ChatLine::Verification {
                artifact_path: existing_path,
                ..
            } => crate::delivery::artifact_key(Path::new(existing_path)) != key,
            _ => true,
        });
    }
    lines.push(card);
}

/// Move the current turn's verification conclusion cards right before trailing
/// artifact cards so the verification receipt stays paired with the deliverable
/// in the viewport without needing to scroll past a long assistant summary.
pub(super) fn promote_latest_turn_verifications(lines: &mut Vec<ChatLine>) {
    let start = lines
        .iter()
        .rposition(|line| matches!(line, ChatLine::User { .. }))
        .map_or(0, |index| index + 1);
    let mut verifications = Vec::new();
    let mut kept = Vec::with_capacity(lines.len().saturating_sub(start));
    for line in lines.drain(start..) {
        if matches!(line, ChatLine::Verification { .. }) {
            upsert_verification_line(&mut verifications, line);
        } else {
            kept.push(line);
        }
    }
    let insert_at = kept
        .iter()
        .rposition(|line| {
            !matches!(
                line,
                ChatLine::Artifact { .. } | ChatLine::TaskReceipt { .. }
            )
        })
        .map_or(0, |index| index + 1);
    let tail = kept.split_off(insert_at);
    lines.extend(kept);
    lines.extend(verifications);
    lines.extend(tail);
}

pub(super) fn parse_verification_line(
    text: &str,
    gate: Option<&crate::delivery::DeliveryStore>,
) -> Option<ChatLine> {
    let overrides = gate.and_then(|store| store.override_index().ok());
    parse_verification_line_with_index(text, overrides.as_ref())
}

fn parse_verification_line_with_index(
    text: &str,
    overrides: Option<&crate::delivery::OverrideIndex>,
) -> Option<ChatLine> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let ok = value.get("ok")?.as_bool()?;
    let artifact_path = value
        .get("artifact")
        .and_then(serde_json::Value::as_str)?
        .to_owned();
    let report_id = value
        .get("report_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let artifact_hash = value
        .get("artifact_hash")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let semantic_hint = value
        .get("semantic_hint")
        .and_then(serde_json::Value::as_str)
        .filter(|hint| !hint.trim().is_empty())
        .map(str::to_owned);
    let issues = value
        .get("issues")
        .and_then(serde_json::Value::as_array)
        .map_or_else(Vec::new, |items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(VerificationIssue {
                        code: item
                            .get("code")
                            .and_then(serde_json::Value::as_str)?
                            .to_owned(),
                        field: item
                            .get("field")
                            .and_then(serde_json::Value::as_str)?
                            .to_owned(),
                    })
                })
                .collect::<Vec<_>>()
        });
    let provenance_warnings = value
        .get("provenance_warnings")
        .and_then(serde_json::Value::as_array)
        .map_or_else(Vec::new, |items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    let override_used = (!report_id.is_empty() || !artifact_hash.is_empty())
        && overrides.is_some_and(|index| {
            index.report_was_overridden(
                &report_id,
                &artifact_path,
                &artifact_hash,
            )
        });
    Some(ChatLine::Verification {
        report_id,
        artifact_path,
        artifact_hash,
        ok,
        issues,
        semantic_hint,
        override_used,
        provenance_warnings,
        source_session_id: None,
    })
}

pub(super) fn lines_from_messages_with_index(
    messages: &[Message],
    allowed: &BTreeSet<String>,
    overrides: Option<&crate::delivery::OverrideIndex>,
) -> Vec<ChatLine> {
    let mut lines = Vec::new();
    let mut pending_verifications = Vec::new();
    let mut pending_artifacts = Vec::new();
    for message in messages {
        match message.role {
            Role::System => {}
            Role::User => {
                if matches!(message.source, MessageSource::User)
                    && !is_internal_resume_message(message)
                {
                    lines.extend(std::mem::take(&mut pending_verifications));
                    for artifact_id in std::mem::take(&mut pending_artifacts) {
                        lines.push(ChatLine::Artifact { artifact_id });
                    }
                }
                if !matches!(message.source, MessageSource::SystemEvent { .. })
                {
                    let text = message
                        .display_text
                        .clone()
                        .unwrap_or_else(|| message.text());
                    if !text.is_empty() {
                        lines.push(ChatLine::User { text });
                    }
                }
                let tool_name = match &message.source {
                    MessageSource::Tool { tool } => Some(tool.as_str()),
                    _ => None,
                };
                for block in &message.content {
                    if let ContentBlock::ToolResult {
                        content,
                        is_error,
                        artifact,
                        ..
                    } = block
                    {
                        let text = content
                            .iter()
                            .filter_map(ContentBlock::as_text)
                            .collect::<Vec<_>>()
                            .join("\n");
                        let errored = is_error.unwrap_or(false);
                        lines.push(ChatLine::ToolResult {
                            text: tool_result_display_text(
                                tool_name.unwrap_or_default(),
                                &text,
                            ),
                            is_error: errored,
                        });
                        if matches!(
                            tool_name,
                            Some("verify_deck" | "verify_artifact")
                        ) && !errored
                            && let Some(card) =
                                parse_verification_line_with_index(
                                    &text, overrides,
                                )
                        {
                            upsert_verification_line(
                                &mut pending_verifications,
                                card,
                            );
                        }
                        if let Some(receipt) = artifact
                            && allowed.contains(&receipt.artifact_id)
                            && !pending_artifacts.contains(&receipt.artifact_id)
                        {
                            pending_artifacts.push(receipt.artifact_id.clone());
                        }
                    }
                }
            }
            Role::Assistant => {
                for block in &message.content {
                    match block {
                        ContentBlock::Text { text } => {
                            if !text.is_empty() {
                                lines.push(ChatLine::Assistant {
                                    text: text.clone(),
                                });
                            }
                        }
                        ContentBlock::Reasoning { text } => {
                            lines.push(ChatLine::Reasoning {
                                text: text.clone(),
                                duration_secs: 0,
                                live: false,
                            });
                        }
                        ContentBlock::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            let arguments = serde_json::from_str(arguments)
                                .unwrap_or(serde_json::Value::Null);
                            lines.push(ChatLine::ToolCall {
                                name: name.clone(),
                                target: tool_target(&arguments),
                                call_id: id.clone(),
                                running: false,
                            });
                        }
                        ContentBlock::ToolResult {
                            content, is_error, ..
                        } => {
                            let text = content
                                .iter()
                                .filter_map(ContentBlock::as_text)
                                .collect::<Vec<_>>()
                                .join("\n");
                            let tool_name = match &message.source {
                                MessageSource::Tool { tool } => tool.as_str(),
                                _ => "",
                            };
                            let errored = is_error.unwrap_or(false);
                            lines.push(ChatLine::ToolResult {
                                text: tool_result_display_text(
                                    tool_name, &text,
                                ),
                                is_error: errored,
                            });
                            if matches!(
                                tool_name,
                                "verify_deck" | "verify_artifact"
                            ) && !errored
                                && let Some(card) =
                                    parse_verification_line_with_index(
                                        &text, overrides,
                                    )
                            {
                                upsert_verification_line(
                                    &mut pending_verifications,
                                    card,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    lines.extend(pending_verifications);
    for artifact_id in pending_artifacts {
        lines.push(ChatLine::Artifact { artifact_id });
    }
    lines
}

pub(super) fn tool_result_display_text(tool_name: &str, text: &str) -> String {
    if tool_name == "publish_file"
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(key) = value.get("ui_key").and_then(|value| value.as_str())
        && key.starts_with("chat.publish.")
    {
        return key.to_owned();
    }
    if matches!(tool_name, "verify_deck" | "verify_artifact")
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(summary) =
            value.get("summary").and_then(|value| value.as_str())
        && !summary.is_empty()
    {
        return summary.to_owned();
    }
    text.to_owned()
}

pub(super) fn live_status(state: TurnState) -> TurnStatus {
    match state {
        TurnState::Idle => TurnStatus::Idle,
        TurnState::WaitingForUser => TurnStatus::WaitingForUser,
        TurnState::Running | TurnState::Stopping | TurnState::Stopped => {
            TurnStatus::Stopped
        }
        TurnState::Completed => TurnStatus::Completed,
        TurnState::Error { message } => TurnStatus::Error { message },
    }
}

pub(super) fn sync_engine_titles(engine: &Engine, state: &UiState) {
    let live = lock(&state.live);
    for (id, session) in live.iter() {
        if session.title.is_empty() {
            continue;
        }
        let Some(record) = engine.session(id) else {
            continue;
        };
        if record.title == session.title && record.titled == session.titled {
            continue;
        }
        if engine
            .set_session_title(id, session.title.clone(), session.titled)
            .is_ok()
        {
            sync_artifact_title(state, engine, id);
        } else {
            state.report_storage_error();
        }
    }
}

pub(super) fn hydrate_engine(
    engine: &Engine,
    state: &UiState,
    config: &LlmConfig,
) {
    let known: std::collections::HashSet<String> = engine
        .sessions()
        .into_iter()
        .map(|record| record.id)
        .collect();
    let live = lock(&state.live);
    for (id, session) in live.iter() {
        if known.contains(id) {
            continue;
        }
        if engine
            .insert_session(SessionRecord {
                id: id.clone(),
                title: session.title.clone(),
                titled: session.titled,
                updated_at: session.updated_at,
                status: TurnState::Stopped,
                messages: messages_from_lines(
                    &session.projection.lines,
                    config,
                ),
                todos: Vec::new(),
                origin: SessionOrigin::Unknown,
                workspace: None,
                delivery_commits: Vec::new(),
                pinned: session.pinned,
                archived: session.archived,
                role: session.role,
                root_session_id: session.root_session_id.clone(),
                access_mode: session.access_mode,
                ..SessionRecord::default()
            })
            .is_err()
        {
            state.report_storage_error();
        }
    }
}

pub(super) fn messages_from_lines(
    lines: &[ChatLine],
    config: &LlmConfig,
) -> Vec<Message> {
    let mut messages = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        match line {
            ChatLine::User { text } if !text.is_empty() => {
                messages.push(Message {
                    id: format!("legacy-u-{index}"),
                    role: Role::User,
                    content: vec![ContentBlock::text(text.clone())],
                    source: MessageSource::User,
                    display_text: None,
                });
            }
            ChatLine::Assistant { text } => messages.push(Message {
                id: format!("legacy-a-{index}"),
                role: Role::Assistant,
                content: vec![ContentBlock::text(text.clone())],
                source: MessageSource::Model {
                    provider: config.provider.clone(),
                    model: config.model.clone(),
                },
                display_text: None,
            }),
            _ => {}
        }
    }
    messages
}

/// Cache key for one tool call: call ids repeat across sessions.
pub(super) fn tool_arg_key(session: &str, call_id: &str) -> String {
    format!("{session}\u{1}{call_id}")
}

pub(super) struct FileChangeDraft {
    pub(super) path: String,
    pub(super) kind: FileChangeKind,
    pub(super) added: u32,
    pub(super) removed: u32,
    pub(super) binary: bool,
}

/// Resolve a tool path against the session directory: `write_file` and
/// `edit_file` accept relative paths and resolve them inside the tool, so the
/// raw argument is what reaches this row.
pub(super) fn resolve_change_path(raw: &str, cwd: &Path) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    Some(absolute.to_string_lossy().into_owned())
}

/// Build the change row from the tool arguments, then drop them.
pub(super) fn take_file_change(
    args: &Mutex<HashMap<String, serde_json::Value>>,
    session: &str,
    call_id: &str,
    cwd: &Path,
) -> Option<FileChangeDraft> {
    let value = lock(args).remove(&tool_arg_key(session, call_id))?;
    let path = resolve_change_path(value.get("path")?.as_str()?, cwd)?;
    if let (Some(old), Some(new)) = (
        value.get("old_string").and_then(|v| v.as_str()),
        value.get("new_string").and_then(|v| v.as_str()),
    ) {
        let counted = if is_binary(old) || is_binary(new) {
            None
        } else {
            line_delta(old, new)
        };
        let (added, removed) = counted.unwrap_or((0, 0));
        return Some(FileChangeDraft {
            path,
            kind: FileChangeKind::Update,
            added,
            removed,
            binary: counted.is_none(),
        });
    }
    // `write_file` replaces the whole file, so the previous content is not in
    // the arguments; the card reports the written size and no removals.
    let content = value.get("content")?.as_str()?;
    let binary = is_binary(content);
    Some(FileChangeDraft {
        path,
        kind: FileChangeKind::Add,
        added: if binary {
            0
        } else {
            content.lines().count() as u32
        },
        removed: 0,
        binary,
    })
}

/// Content carrying a NUL byte is not text; counting its lines is meaningless.
pub(super) fn is_binary(text: &str) -> bool {
    text.as_bytes().contains(&0)
}

/// Line counts are a hint on the card, so a very large edit is left uncounted
/// instead of spending seconds inside the diff on the turn path.
const MAX_DIFF_BYTES: usize = 200 * 1024;

/// Added and removed line counts for one change; `None` when the edit is too
/// large to count cheaply.
pub(super) fn line_delta(old: &str, new: &str) -> Option<(u32, u32)> {
    if old.len().saturating_add(new.len()) > MAX_DIFF_BYTES {
        return None;
    }
    use similar::{ChangeTag, TextDiff};
    let mut added = 0u32;
    let mut removed = 0u32;
    for change in TextDiff::from_lines(old, new).iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }
    Some((added, removed))
}
