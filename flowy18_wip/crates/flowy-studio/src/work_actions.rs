//! Visual Board, Flowchart Modal, local environment, popover reset, and
//! decoupled task control actions (`cancel_task` / `apply_card_action`) on
//! `StudioPanel`.

use flowy_agent::{
    CancelOutcome, CardAction, CardActionOutcome, ReceiptState, TaskTarget,
};
use gpui::Context;

use crate::panel::{ActivePopover, StudioPanel, StudioSurface};

/// Validates and opens a local file in the OS default application.
///
/// Checks `can_open_with_default_app` both before and after canonical local
/// file resolution (closing TOCTOU symlink-to-executable swap windows).
pub fn open_safe_local_file(path: &str) -> Result<(), &'static str> {
    let safe_path = flowy_agent::can_open_with_default_app(path)
        .then(|| crate::ports::resolve_safe_local_file(path))
        .flatten()
        .filter(|canon| {
            canon
                .to_str()
                .is_some_and(flowy_agent::can_open_with_default_app)
        })
        .ok_or("library.error.open")?;
    open::that(&safe_path).map_err(|_| "library.error.open")
}

impl StudioPanel {
    /// Synchronizes both claimable pending receipts (for legacy tests/counts)
    /// and all non-terminal visible receipts (`Queued`/`Running`/`Retrying`/
    /// `Quarantined`/`Dead`) from the kernel's durable journal.
    pub(crate) fn sync_receipts(&mut self) {
        self.pending_receipts = self.agent.pending_acceptances();
        self.visible_receipts = self.agent.visible_acceptances();
        self.refresh_popover_works();
    }

    /// Whether `path` is an existing safe local regular file allowed to launch
    /// in the OS default application (blocks executables/scripts, UNC shares,
    /// device namespaces, symlinks, and NTFS reparse points).
    #[cfg(test)]
    #[must_use]
    pub(crate) fn can_open_file_externally(path: &str) -> bool {
        flowy_agent::can_open_with_default_app(path)
            && crate::ports::resolve_safe_local_file(path).is_some_and(
                |canon| {
                    canon
                        .to_str()
                        .is_some_and(flowy_agent::can_open_with_default_app)
                },
            )
    }

    /// Opens `path` in the OS default application after verifying the
    /// safe-extension allowlist before AND after resolving the canonical local
    /// file path (closes TOCTOU symlink-to-executable swap windows).
    pub(crate) fn open_file_externally(
        &mut self,
        path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if let Err(notice) = open_safe_local_file(path) {
            self.emit_notice(Some(notice), cx);
        }
    }

    /// Clears cloud credentials, stops any in-flight submission, applies the
    /// disconnected config (locking out autonomous drain), synchronously
    /// cancels all running sessions and non-terminal receipts, and resets UI v2
    /// overlays, filters, cache, and retry state on sign-out.
    pub fn prepare_for_sign_out(
        &mut self,
        disconnected: flowy_agent::LlmConfig,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(stop) = &self.submission_stop {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        self.invalidate_submission(cx);
        self.cloud_ready = false;
        self.catalog_loading = false;
        self.catalog = flowy_agent::ProviderCatalog::new();
        self.tiers.clear();
        self.plan = flowy_agent::Plan::Free;
        self.credits = crate::CreditsViewState::Unavailable;
        self.selected_skill = None;
        self.board_project_filter = None;
        self.board_day_filter = None;
        self.board_format_filter = None;
        self.retrying_receipts.clear();
        self.pending_card_selected.clear();
        self.resolving_cards.clear();
        self.retrying_delivery = None;
        self.close_all_popovers();
        self.board_inspector.reset();
        self.clear_right_sheet();
        self.cached_board_lines = None;
        self.cached_trace = None;
        // Apply disconnected config first so autonomous drain cannot claim new
        // receipts while we synchronously cancel in-flight work.
        let _ = self.agent.apply_config(disconnected);
        let mut busy = self.agent.busy_session_ids();
        if busy.is_empty()
            && let Some(active) = self.agent.active_session_id()
        {
            busy.push(active);
        }
        for session_id in busy {
            let _ = self.agent.cancel_task(TaskTarget::Session(session_id));
        }
        for (receipt, state) in self.agent.visible_acceptances() {
            if matches!(
                state,
                ReceiptState::Queued
                    | ReceiptState::Running
                    | ReceiptState::Retrying
            ) {
                let _ = self.agent.cancel_task(TaskTarget::Receipt(receipt.id));
            }
        }
        self.sync_receipts();
        self.refresh_local_env();
        self.refresh_board_snapshot();
        cx.notify();
    }

    /// Rebinds new sessions to the default workspace without discarding the
    /// user's in-progress composer draft.
    pub(crate) fn bind_default_workspace(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        if self.active_project_id.is_none()
            && self.agent.active_session_id().is_none()
        {
            self.apply_draft_project(None);
            self.refresh_local_env();
            cx.notify();
            return;
        }
        let clear_error = self.agent.clear_active().err();
        self.invalidate_submission(cx);
        self.clear_right_sheet();
        self.apply_draft_project(None);
        if self.surface == StudioSurface::Chat {
            self.surface = StudioSurface::Home;
            self.refresh_chat(cx);
        }
        self.refresh_popover_works();
        self.refresh_local_env();
        self.emit_notice(clear_error, cx);
        cx.emit(crate::panel::StudioEvent::SessionsChanged);
        cx.notify();
    }

    /// Resolves whichever session (or synthetic unassigned bucket) is currently
    /// displaying the Trace tab across the Board Inspector or Chat Right Drawer.
    #[must_use]
    pub(crate) fn active_trace_session_id(&self) -> Option<String> {
        if self.surface == StudioSurface::Board
            && self.board_inspector.tab
                == crate::overview::BoardInspectorTab::Trace
        {
            return self
                .board_inspector
                .worker_session
                .clone()
                .or_else(|| self.board_inspector.session.clone());
        }
        if self.right_drawer_open
            && self.drawer_tab == crate::deliverables::RightDrawerTab::Trace
        {
            return match &self.inspected_work {
                Some(crate::deliverables::InspectedWork::Session {
                    id,
                    ..
                }) => Some(id.clone()),
                Some(crate::deliverables::InspectedWork::Receipt(r)) => Some(
                    r.target_session_id
                        .clone()
                        .or_else(|| r.session_hint.clone())
                        .unwrap_or_else(|| {
                            crate::ports::UNASSIGNED_QUEUE_SESSION_ID.to_owned()
                        }),
                ),
                None => self.agent.active_session_id(),
            };
        }
        None
    }

    /// Recomputes the dirty-cached structured chat lines for the currently
    /// inspected Board session/worker so `Render::render` stays lock-free.
    /// Skips cloning `Vec<ChatLine>` unless the Board `Timeline` tab is active.
    pub(crate) fn refresh_board_lines_cache(&mut self) {
        if self.surface != StudioSurface::Board
            || self.board_inspector.tab
                != crate::overview::BoardInspectorTab::Timeline
        {
            self.cached_board_lines = None;
            return;
        }
        let Some(target_sid) = self
            .board_inspector
            .worker_session
            .as_deref()
            .or(self.board_inspector.session.as_deref())
        else {
            self.cached_board_lines = None;
            return;
        };
        if target_sid == crate::ports::UNASSIGNED_QUEUE_SESSION_ID {
            self.cached_board_lines = None;
            return;
        }
        let lines = self.agent.session_lines(target_sid);
        self.cached_board_lines = Some((target_sid.to_owned(), lines));
    }

    /// Recomputes the dirty-cached redacted Trace rows for the open Trace tab
    /// (never called inside `Render::render`).
    pub(crate) fn refresh_trace_cache(&mut self) {
        let Some(session_id) = self.active_trace_session_id() else {
            self.cached_trace = None;
            return;
        };
        let entries = crate::ports::collect_session_traces(
            &self.agent,
            &session_id,
            &self.visible_receipts,
        );
        self.cached_trace = Some((session_id, entries));
    }

    /// Overlay scope identifier of the currently open Studio popover, if any.
    #[must_use]
    pub const fn active_popup_scope(&self) -> Option<&'static str> {
        self.popover.scope()
    }

    /// Dismisses whichever Studio popover or Board narrow drawer matches `scope`.
    /// Returns `true` when state changed and notified observers.
    pub fn dismiss_overlay(
        &mut self,
        scope: &str,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        if scope == "board-inspector-drawer" {
            if !self.has_board_drawer() {
                return false;
            }
            self.close_board_drawer(cx);
            return true;
        }
        if self.popover.scope() == Some(scope) {
            self.close_all_popovers();
            cx.notify();
            return true;
        }
        false
    }

    /// Closes all mutually exclusive transient popovers in one place so new
    /// cockpit menus cannot be missed by navigation or dialog transitions.
    pub fn close_all_popovers(&mut self) {
        self.popover = ActivePopover::None;
        self.credits_close = None;
    }

    pub(crate) fn toggle_popover(&mut self, next: ActivePopover) {
        self.credits_close = None;
        self.popover = if self.popover == next {
            ActivePopover::None
        } else {
            next
        };
    }

    pub(crate) fn open_popover(&mut self, next: ActivePopover) {
        self.credits_close = None;
        self.popover = next;
    }

    #[must_use]
    pub(crate) const fn is_model_menu_open(&self) -> bool {
        matches!(self.popover, ActivePopover::ModelMenu)
    }

    #[must_use]
    pub(crate) const fn is_bg_tasks_open(&self) -> bool {
        matches!(self.popover, ActivePopover::BgTasks)
    }

    #[must_use]
    pub(crate) const fn is_add_menu_open(&self) -> bool {
        matches!(self.popover, ActivePopover::AddMenu)
    }

    pub(crate) fn cancel_task_target(
        &self,
        target: TaskTarget,
        cx: &Context<'_, Self>,
    ) {
        let agent = self.agent.clone();
        let job = cx.background_executor().spawn(async move {
            let outcome = agent.cancel_task(target);
            let updates = agent.drain_updates();
            (outcome, updates)
        });
        cx.spawn(async move |weak, cx| {
            let (outcome, updates) = job.await;
            let _ = weak.update(cx, |panel, cx| {
                panel.process_agent_updates(updates, cx);
                match outcome {
                    Ok(
                        CancelOutcome::Cancelled
                        | CancelOutcome::AlreadySettled,
                    ) => {
                        panel.sync_receipts();
                        panel.refresh_chat(cx);
                        panel.refresh_board_snapshot();
                    }
                    Ok(CancelOutcome::FinalizingNotCancellable) => {
                        panel.emit_notice(Some("notice.cancel_finalizing"), cx);
                    }
                    Err(error) => {
                        panel.emit_notice(Some(error), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Requests a graceful stop for one specific session, routing strictly by
    /// session role (`Worker` -> `cancel_work`, `Router` -> `stop_router_reply`,
    /// `Standard` -> `cancel_task`) without coupling to the focused session (Q6).
    pub(crate) fn stop_session_turn(
        &self,
        session_id: &str,
        cx: &Context<'_, Self>,
    ) {
        match self.agent.session_role(session_id) {
            flowy_agent::SessionRole::Worker => {
                let root_id = self.agent.root_session_id(session_id);
                self.cancel_child_work(&root_id, session_id, cx);
            }
            flowy_agent::SessionRole::Router => {
                self.stop_router_reply_target(session_id, cx);
            }
            flowy_agent::SessionRole::Standard => {
                self.cancel_task_target(
                    TaskTarget::Session(session_id.to_owned()),
                    cx,
                );
            }
        }
    }

    fn run_stop_work_action<F>(
        &self,
        sync_receipts: bool,
        op: F,
        cx: &Context<'_, Self>,
    ) where
        F: FnOnce(
                &flowy_agent::AgentHandle,
            )
                -> Result<flowy_agent::StopWorkResult, &'static str>
            + Send
            + 'static,
    {
        let agent = self.agent.clone();
        let job = cx.background_executor().spawn(async move {
            let outcome = op(&agent);
            let updates = agent.drain_updates();
            (outcome, updates)
        });
        cx.spawn(async move |weak, cx| {
            let (outcome, updates) = job.await;
            let _ = weak.update(cx, |panel, cx| {
                panel.process_agent_updates(updates, cx);
                match outcome {
                    Ok(res) if res.outcome == "finalizing_not_cancellable" => {
                        panel.emit_notice(Some("notice.cancel_finalizing"), cx);
                    }
                    Ok(_) => {
                        if sync_receipts {
                            panel.sync_receipts();
                        }
                        panel.refresh_chat(cx);
                        panel.refresh_board_snapshot();
                    }
                    Err(error) => {
                        panel.emit_notice(Some(error), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stops ONLY the Router's active reply without cancelling child Workers or
    /// discarding queued follow-ups (Q6).
    pub(crate) fn stop_router_reply_target(
        &self,
        router_id: &str,
        cx: &Context<'_, Self>,
    ) {
        let rid = router_id.to_owned();
        self.run_stop_work_action(
            false,
            move |agent| agent.stop_router_reply(&rid),
            cx,
        );
    }

    /// Cancels one specific child Worker owned by `root_session_id`, leaving
    /// the parent Router and all sibling Workers untouched (Q6).
    pub(crate) fn cancel_child_work(
        &self,
        root_session_id: &str,
        worker_id: &str,
        cx: &Context<'_, Self>,
    ) {
        let rid = root_session_id.to_owned();
        let wid = worker_id.to_owned();
        self.run_stop_work_action(
            true,
            move |agent| agent.cancel_work(&rid, &wid),
            cx,
        );
    }

    /// Opens the parent Router conversation (if not already focused) and opens
    /// the right-hand inspector sheet pinned to child `worker_id` (Q2).
    pub(crate) fn open_child_work_in_parent(
        &mut self,
        root_session_id: &str,
        worker_id: &str,
        title: String,
        cx: &mut Context<'_, Self>,
    ) {
        if self.agent.active_session_id().as_deref() != Some(root_session_id)
            || self.surface != StudioSurface::Chat
        {
            self.open_session(root_session_id, cx);
        }
        self.open_work_inspector(
            crate::deliverables::InspectedWork::Session {
                id: worker_id.to_owned(),
                title,
            },
            cx,
        );
    }

    /// Cancels a queued or active deferred acceptance receipt directly.
    pub(crate) fn cancel_receipt(
        &self,
        receipt_id: &str,
        cx: &Context<'_, Self>,
    ) {
        self.cancel_task_target(TaskTarget::Receipt(receipt_id.to_owned()), cx);
    }

    /// Requeues a `Dead` receipt through the kernel's unified card-action
    /// gateway; the kernel supersedes the old dead receipt automatically.
    pub(crate) fn retry_dead_receipt(
        &mut self,
        receipt_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.retrying_receipts.insert(receipt_id.to_owned()) {
            return;
        }
        cx.notify();
        let agent = self.agent.clone();
        let target_id = receipt_id.to_owned();
        let job_id = target_id.clone();
        let job = cx.background_executor().spawn(async move {
            let result =
                agent.apply_card_action(CardAction::RetryDeadReceipt {
                    receipt_id: job_id,
                });
            let updates = agent.drain_updates();
            (result, updates)
        });
        cx.spawn(async move |weak, cx| {
            let (result, updates) = job.await;
            let _ = weak.update(cx, |panel, cx| {
                panel.retrying_receipts.remove(target_id.as_str());
                panel.process_agent_updates(updates, cx);
                match result {
                    Ok(CardActionOutcome::ReceiptRequeued { .. }) => {
                        panel.sync_receipts();
                        panel.refresh_board_snapshot();
                        panel.emit_notice(None, cx);
                    }
                    Ok(_) => {
                        panel.sync_receipts();
                        panel.refresh_board_snapshot();
                    }
                    Err(error) => panel.emit_notice(Some(error), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Forces an immediate board snapshot rebuild and notifies observers.
    pub fn refresh_board_now(&mut self, cx: &mut Context<'_, Self>) {
        self.refresh_board_snapshot();
        cx.notify();
    }

    /// Rebuilds the cached Visual Board snapshot from `AgentHandle`.
    /// Called strictly on structural `AgentUpdate`, project changes, or opening
    /// the Board — never per-token or inside `Render::render`.
    pub(crate) fn refresh_board_snapshot(&mut self) {
        use crate::ports::BoardDataSource as _;
        self.board_generation = self.board_generation.wrapping_add(1);
        let default_label = self.t("studio.space_default_sub");
        let unassigned_title = self.t("chat.bg_tasks_title");
        let i18n = self.i18n.clone();
        self.board_snapshot = self.agent.build_board_snapshot(
            &self.projects,
            &self.visible_receipts,
            &default_label,
            &unassigned_title,
            self.board_generation,
            &|key| i18n.t(key),
        );
        if let Some(selected_day) = self.board_day_filter
            && !self
                .board_snapshot
                .daily_7d
                .iter()
                .any(|bucket| bucket.date_key == selected_day)
        {
            self.board_day_filter = None;
        }
        if let Some(selected_sid) = self.board_inspector.session.as_deref() {
            match self
                .board_snapshot
                .sessions
                .iter()
                .find(|group| group.session_id == selected_sid)
            {
                None => self.board_inspector.reset(),
                Some(group) => {
                    if let Some(bid) = self.board_inspector.branch.as_deref()
                        && !group.branches.iter().any(|b| b.id == bid)
                        && !group.workspace_changes.iter().any(|b| b.id == bid)
                    {
                        self.board_inspector.branch = None;
                    }
                    if let Some(wid) =
                        self.board_inspector.worker_session.as_deref()
                        && !group
                            .child_workers
                            .iter()
                            .any(|w| w.session_id == wid)
                    {
                        self.board_inspector.worker_session = None;
                    }
                }
            }
        }
        if self.surface == StudioSurface::Board {
            self.ensure_board_selection();
        }
        self.refresh_popover_works();
        if self.surface == StudioSurface::Board {
            self.refresh_board_lines_cache();
        }
        self.refresh_trace_cache();
        self.last_board_refresh = Some(std::time::Instant::now());
    }

    fn ensure_board_selection(&mut self) {
        if let Some(sid) = self.board_inspector.session.as_deref()
            && self
                .board_snapshot
                .sessions
                .iter()
                .any(|g| g.session_id == sid)
        {
            return;
        }
        let proj_filter = self.board_project_filter.as_deref();
        let next = pick_default_board_session(
            &self.board_snapshot.sessions,
            proj_filter,
        )
        .map(str::to_owned);
        self.board_inspector.session = next;
        self.board_inspector.branch = None;
        self.board_inspector.worker_session = None;
        self.board_inspector.override_ack_key = None;
        self.board_inspector.workspace_changes_expanded = false;
    }

    /// Rebuilds the cached local environment health snapshot (never called per
    /// render frame).
    pub(crate) fn refresh_local_env(&mut self) {
        let model_ready = self.agent.can_run_turns()
            && (self.cloud_ready || !self.agent.requires_cloud_ready());
        self.local_env = crate::ports::probe_local_env(
            &self.projects,
            &self.installed_skills,
            model_ready,
            self.agent.semantic_verify_enabled(),
        );
    }

    /// Public entry point for `flowy-app` after mutating `cloud_ready`.
    pub fn refresh_env_status(&mut self, cx: &mut Context<'_, Self>) {
        self.refresh_local_env();
        cx.notify();
    }

    /// Opens the Global Visual Board (`StudioSurface::Board`).
    pub fn open_board(&mut self, cx: &mut Context<'_, Self>) {
        self.board_project_filter = None;
        self.board_day_filter = None;
        self.board_format_filter = None;
        self.board_inspector.close_drawer();
        self.drawer_preview.clear();
        self.clear_right_sheet();
        self.close_all_popovers();
        self.surface = StudioSurface::Board;
        self.refresh_board_snapshot();
        self.ensure_board_selection();
        self.refresh_board_lines_cache();
        self.refresh_trace_cache();
        cx.notify();
    }

    /// Opens the Global Visual Board pre-filtered to one local project.
    pub fn open_board_for_project(
        &mut self,
        project_id: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.open_board(cx);
        self.board_project_filter = project_id;
        self.board_inspector.session = None;
        self.ensure_board_selection();
        self.refresh_board_lines_cache();
        self.refresh_trace_cache();
        cx.notify();
    }

    /// Selects a session inside the Master-Detail Board Inspector (and opens
    /// the narrow slide drawer when `open_drawer` is true).
    pub(crate) fn select_board_session(
        &mut self,
        session_id: String,
        branch_id: Option<String>,
        open_drawer: bool,
        cx: &mut Context<'_, Self>,
    ) {
        self.close_all_popovers();
        if !self
            .board_snapshot
            .sessions
            .iter()
            .any(|g| g.session_id == session_id)
        {
            self.refresh_board_snapshot();
        }
        let exists = self
            .board_snapshot
            .sessions
            .iter()
            .any(|g| g.session_id == session_id);
        if !exists {
            self.board_inspector.reset();
            self.drawer_preview.clear();
            self.cached_board_lines = None;
            self.refresh_trace_cache();
            cx.notify();
            return;
        }
        let changed_session = self.board_inspector.session.as_deref()
            != Some(session_id.as_str());
        if changed_session {
            self.drawer_preview.clear();
            self.board_inspector.worker_session = None;
            self.board_inspector.override_ack_key = None;
            self.board_inspector.workspace_changes_expanded = false;
        }
        self.board_inspector.session = Some(session_id);
        self.board_inspector.branch = branch_id;
        if open_drawer {
            self.board_inspector.drawer_open = true;
        }
        self.refresh_board_lines_cache();
        self.refresh_trace_cache();
        cx.notify();
    }

    pub(crate) fn select_board_tab(
        &mut self,
        tab: crate::overview::BoardInspectorTab,
        cx: &mut Context<'_, Self>,
    ) {
        if self.board_inspector.tab != tab {
            self.board_inspector.tab = tab;
            if tab != crate::overview::BoardInspectorTab::Overview {
                self.drawer_preview.clear();
            }
            self.refresh_board_lines_cache();
            self.refresh_trace_cache();
            cx.notify();
        }
    }

    pub(crate) fn select_board_worker(
        &mut self,
        worker_session: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.board_inspector.worker_session != worker_session {
            self.board_inspector.worker_session = worker_session;
            self.refresh_board_lines_cache();
            self.refresh_trace_cache();
            cx.notify();
        }
    }

    /// Closes the narrow-width Board slide drawer cleanly.
    pub fn close_board_drawer(&mut self, cx: &mut Context<'_, Self>) {
        if self.board_inspector.drawer_open {
            self.board_inspector.close_drawer();
            self.refresh_trace_cache();
            cx.notify();
        }
    }

    /// Resolves the currently selected session group for the Board Inspector.
    #[must_use]
    pub(crate) fn resolve_board_inspector(
        &self,
    ) -> Option<&crate::ports::SessionBoardGroup> {
        let session_id = self.board_inspector.session.as_deref()?;
        self.board_snapshot
            .sessions
            .iter()
            .find(|g| g.session_id == session_id)
    }

    /// Whether the Board narrow-screen slide drawer is currently visible.
    #[must_use]
    pub fn has_board_drawer(&self) -> bool {
        self.surface == StudioSurface::Board
            && self.board_width_px < crate::overview::BOARD_SPLIT_MIN_W
            && self.board_inspector.has_drawer()
            && self.resolve_board_inspector().is_some()
    }

    /// Jumps straight to the first session waiting on user approval, delivery
    /// retry, or dead/quarantined receipt recovery.
    pub(crate) fn jump_to_first_blocker(
        &mut self,
        open_drawer: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let proj_filter = self.board_project_filter.as_deref();
        let found = self
            .board_snapshot
            .sessions
            .iter()
            .filter(|group| {
                !group.archived
                    && group.has_blocker
                    && proj_filter.is_none_or(|pid| {
                        group.project_id.as_deref() == Some(pid)
                    })
            })
            .map(|group| {
                let bid = group
                    .branches
                    .iter()
                    .find(|branch| branch.phase.is_blocker())
                    .map(|branch| branch.id.clone());
                (group.session_id.clone(), bid)
            })
            .next();
        if let Some((sid, bid)) = found {
            self.board_inspector.tab =
                crate::overview::BoardInspectorTab::Overview;
            self.select_board_session(sid, bid, open_drawer, cx);
        } else {
            self.emit_notice(Some("board.no_blockers"), cx);
        }
    }

    /// Retries a failed durable delivery commit for `session_id` on a
    /// background worker (callable from both Chat and Board Inspector).
    pub(crate) fn retry_delivery_for_session(
        &mut self,
        session_id: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if self.retrying_delivery.is_some() {
            return;
        }
        let target = session_id.to_owned();
        let agent = self.agent.clone();
        self.retrying_delivery = Some(target.clone());
        self.refresh_working(cx);
        cx.notify();
        let job_target = target.clone();
        let job = cx.background_executor().spawn(async move {
            let result = agent.apply_card_action(CardAction::RetryDelivery {
                session_id: job_target,
                submission_id: None,
            });
            let updates = agent.drain_updates();
            (result, updates)
        });
        cx.spawn(async move |weak, cx| {
            let (result, updates) = job.await;
            let _ = weak.update(cx, |panel, cx| {
                if panel.retrying_delivery.as_deref() == Some(target.as_str()) {
                    panel.retrying_delivery = None;
                }
                panel.process_agent_updates(updates, cx);
                if let Err(error) = result
                    && error != "chat.status_delivery_failed"
                {
                    panel.emit_notice(Some(error), cx);
                }
                panel.refresh_chat(cx);
                panel.refresh_board_snapshot();
                panel.refresh_working(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Pushes installed skills from `flowy-app` (`flowy-skills`) into Studio.
    pub fn set_installed_skills(
        &mut self,
        skills: Vec<crate::ports::SkillChip>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.installed_skills != skills {
            self.installed_skills = skills;
            if let Some(selected) = &self.selected_skill {
                self.selected_skill = self
                    .installed_skills
                    .iter()
                    .find(|chip| chip.id == selected.id)
                    .cloned();
            }
            self.refresh_local_env();
            cx.notify();
        }
    }

    /// Cached count of sessions waiting for explicit user approval or recovery.
    #[must_use]
    pub const fn board_blocker_count(&self) -> usize {
        self.board_snapshot.kpi.blocker_sessions
    }

    /// Cached count of running/active sessions for dock badge alignment with
    /// `total_active_work_count`.
    #[must_use]
    pub const fn board_active_count(&self) -> usize {
        self.board_snapshot.kpi.running_sessions
    }

    /// Toggles the kernel L3 semantic verification disclosure switch.
    pub(crate) fn toggle_semantic_verify(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        let next = !self.agent.semantic_verify_enabled();
        self.agent.set_semantic_verify_enabled(next);
        self.refresh_local_env();
        cx.notify();
    }

    /// Registers the parent directory of a delivered file as a persistent local
    /// project in the sidebar (rejecting internal `store_root` directories,
    /// filesystem roots, UNC shares, and symlinks / NTFS reparse points).
    pub(crate) fn register_deliverable_folder_as_project(
        &mut self,
        file_path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(parent) = crate::ports::resolve_safe_parent_dir(file_path)
        else {
            self.emit_notice(Some("project.invalid_folder"), cx);
            return;
        };
        if parent.parent().is_none()
            || crate::ports::is_same_or_overlapping_dir(
                &parent,
                &self.store_root,
            )
        {
            self.emit_notice(Some("project.invalid_folder"), cx);
            return;
        }
        let name = parent
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Flowy")
            .to_owned();
        match self.create_project(&name, &parent, cx) {
            Ok(_) => {
                self.refresh_board_snapshot();
                self.emit_notice(Some("deliverables.promoted_project"), cx);
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }
}

/// Picks the default selected session in the Board Master-Detail Inspector,
/// preferring the first non-synthetic blocker at the top of the action queue.
fn pick_default_board_session<'a>(
    sessions: &'a [crate::ports::SessionBoardGroup],
    project_filter: Option<&str>,
) -> Option<&'a str> {
    let matches_scope = |g: &&crate::ports::SessionBoardGroup| {
        !g.archived
            && project_filter
                .is_none_or(|pid| g.project_id.as_deref() == Some(pid))
    };
    let preds: [fn(&crate::ports::SessionBoardGroup) -> bool; 4] = [
        |g| !g.is_synthetic() && g.has_blocker,
        |g| g.has_blocker,
        |g| g.is_running,
        |_| true,
    ];
    preds
        .into_iter()
        .find_map(|pred| {
            sessions.iter().filter(matches_scope).find(|g| pred(g))
        })
        .or_else(|| sessions.first())
        .map(|g| g.session_id.as_str())
}

#[cfg(test)]
mod board_selection_tests {
    use super::pick_default_board_session;
    use crate::ports::{
        BoardKpiSummary, SessionBoardGroup, UNASSIGNED_QUEUE_SESSION_ID,
    };

    fn group(id: &str, blocker: bool, running: bool) -> SessionBoardGroup {
        SessionBoardGroup {
            session_id: id.into(),
            title: id.into(),
            project_id: None,
            project_name: None,
            workspace_display: String::new(),
            updated_at: 1_700_000_000_000,
            pinned: false,
            archived: false,
            preview: None,
            assistant_summary: None,
            branches: Vec::new(),
            workspace_changes: Vec::new(),
            child_workers: Vec::new(),
            total_branch_count: 0,
            total_file_count: 0,
            session_contract: None,
            has_blocker: blocker,
            has_delivered: false,
            is_running: running,
            is_active: blocker || running,
            group_kpi: BoardKpiSummary::default(),
        }
    }

    #[test]
    fn default_board_selection_prefers_first_real_blocker_over_queue() {
        let sessions = vec![
            group("s-first-blocker", true, false),
            group(UNASSIGNED_QUEUE_SESSION_ID, true, true),
            group("s-second-blocker", true, false),
        ];
        assert_eq!(
            pick_default_board_session(&sessions, None),
            Some("s-first-blocker")
        );
    }
}
