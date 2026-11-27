use super::{ActivePopover, StudioEvent, StudioPanel, StudioSurface};
use flowy_agent::{AgentWorkspace, CardAction, ReceiptState};
use flowy_core::DocumentSession;
use gpui::Context;

impl StudioPanel {
    pub fn set_surface(
        &mut self,
        surface: StudioSurface,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(surface, StudioSurface::Chat) {
            self.session_opened_by_user = true;
        }
        if self.surface != surface {
            self.invalidate_submission(cx);
            self.surface = surface;
            self.refresh_popover_works();
            cx.notify();
        }
    }

    pub fn reset_session(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(current) = self.agent.active_session_id() {
            self.draft_quotes.remove(&current);
        }
        self.active_quote = None;
        self.quote_session = None;
        self.invalidate_submission(cx);
        self.close_all_popovers();
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.document = DocumentSession::new(self.product);
        let _ = self.agent.clear_active();
        self.apply_draft_project(None);
        self.surface = StudioSurface::Home;
        self.refresh_chat(cx);
        self.refresh_board_snapshot();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    pub fn new_task(&mut self, cx: &mut Context<'_, Self>) {
        let inherited_project = self
            .active_project_id
            .as_deref()
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned();
        match self.agent.clear_active() {
            Ok(()) => {
                self.invalidate_submission(cx);
                self.close_all_popovers();
                self.board_inspector.close_drawer();
                self.clear_right_sheet();
                self.apply_draft_project(inherited_project.as_ref());
                self.surface = StudioSurface::Home;
                self.refresh_chat(cx);
                self.refresh_board_snapshot();
                self.emit_notice(None, cx);
                self.set_prompt_text("", cx);
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }

    /// Present a global-home state after the host has cleared the active
    /// session and prepared its origin on a background worker.
    pub fn show_new_home(&mut self, cx: &mut Context<'_, Self>) {
        self.invalidate_submission(cx);
        self.close_all_popovers();
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.active_project_id = None;
        self.document = DocumentSession::new(self.product);
        self.submission = None;
        self.surface = StudioSurface::Home;
        self.set_prompt_text("", cx);
        self.refresh_chat(cx);
        self.refresh_board_snapshot();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    /// Toggles the floating background-tasks popover anchored above the composer.
    pub fn toggle_bg_tasks(&mut self, cx: &mut Context<'_, Self>) {
        self.toggle_popover(ActivePopover::BgTasks);
        if self.is_bg_tasks_open() {
            self.refresh_popover_works();
        }
        cx.notify();
    }

    /// Running sessions shown in the background-works popover: on Chat,
    /// excludes the active foreground session (already surfaced by the inline
    /// process capsule and `WorkingIndicator`); on Home, includes all running
    /// sessions so folded work stays reachable.
    #[must_use]
    pub(crate) fn background_running_session_ids(&self) -> Vec<String> {
        let active = self.agent.active_session_id();
        let exclude_active = self.surface == StudioSurface::Chat;
        let mut ids: Vec<String> = self
            .agent
            .busy_session_ids()
            .into_iter()
            .filter(|id| {
                !(exclude_active && active.as_deref() == Some(id.as_str()))
            })
            .collect();
        if !exclude_active
            && ids.is_empty()
            && (self.status.is_busy() || self.submission_pending)
            && let Some(active_id) = active
        {
            ids.push(active_id);
        }
        ids
    }

    /// Visible receipts shown in the background-tasks popover, skipping any
    /// `Running` receipt whose target session is already represented in
    /// `running_ids` so running work is never listed twice.
    pub(crate) fn popover_visible_receipts<'a>(
        &'a self,
        running_ids: &[String],
    ) -> Vec<(&'a flowy_agent::AcceptedMessage, ReceiptState)> {
        self.visible_receipts
            .iter()
            .filter(|(receipt, state)| {
                if *state == ReceiptState::Running {
                    let already_listed =
                        receipt.target_session_id.as_deref().is_some_and(
                            |sid| running_ids.iter().any(|id| id == sid),
                        );
                    return !already_listed;
                }
                !matches!(
                    state,
                    ReceiptState::Completed | ReceiptState::Cancelled
                )
            })
            .map(|(receipt, state)| (receipt, *state))
            .collect()
    }

    /// Total active/attention/queued works shown in the background-tasks pill
    /// and Home resume pill (backed by the cached `popover_works` snapshot).
    #[must_use]
    pub fn background_work_count(&self) -> usize {
        self.popover_total_count()
    }

    /// Total active/queued works across all sessions (including foreground),
    /// for the sidebar Chief Assistant status badge.
    #[must_use]
    pub fn total_active_work_count(&self) -> usize {
        let busy_ids = self.agent.busy_session_ids();
        let mut running = busy_ids.len();
        if (self.status.is_busy() || self.submission_pending) && running == 0 {
            running = 1;
        }
        running + self.popover_visible_receipts(&busy_ids).len()
    }

    /// Jumps to the Chief Assistant's active default-workspace thread, or lands
    /// calmly on the Home stage ready for a fresh instruction.
    pub fn open_chief_assistant(&mut self, cx: &mut Context<'_, Self>) {
        self.close_all_popovers();
        let sessions = self.agent.list_sessions();
        let active_is_live_default = self
            .agent
            .active_session_id()
            .as_deref()
            .is_some_and(|active_id| {
                sessions.iter().any(|s| {
                    s.id == active_id && !s.archived && s.project_id.is_none()
                })
            });
        if active_is_live_default
            && self.active_project_id.is_none()
            && self.agent.has_messages()
        {
            self.session_opened_by_user = true;
            self.surface = StudioSurface::Chat;
            self.refresh_chat(cx);
            self.refresh_popover_works();
            cx.notify();
            return;
        }
        let latest_default = sessions
            .into_iter()
            .find(|session| session.project_id.is_none() && !session.archived);
        if let Some(session) = latest_default {
            self.open_session(&session.id, cx);
        } else {
            self.reset_session(cx);
        }
    }

    /// Marks the cloud route ready and refreshes receipts/board state.
    /// In autonomous mode, `AgentHandle::apply_config` wakes draining itself;
    /// deterministic unit tests still use the legacy check below.
    pub fn notify_cloud_ready(&mut self, cx: &mut Context<'_, Self>) {
        self.cloud_ready = true;
        self.refresh_local_env();
        if self.agent.autonomous_drain() {
            self.sync_receipts();
            self.refresh_board_snapshot();
            cx.notify();
        } else if self.initialized {
            self.refresh_pending_receipts(cx);
        } else {
            cx.notify();
        }
    }

    /// Re-read the acceptance journal, finish a settled claim, and start the
    /// next receipt when the agent is idle.
    pub(super) fn refresh_pending_receipts(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        // A manual submit in flight must not interleave with a queued receipt;
        // the pump re-checks when it settles.
        if self.submission_pending {
            self.sync_receipts();
            cx.notify();
            return;
        }
        if self.agent.has_active_claim() {
            let claimed_status = self.agent.claimed_session_status();
            if claimed_status
                .as_ref()
                .is_some_and(flowy_agent::TurnStatus::is_busy)
            {
                self.sync_receipts();
                cx.notify();
                return;
            }
            // Completed and DeliveryFailed already finished their LLM work
            // (DeliveryFailed has its own Retry Delivery surface). Stopped
            // (e.g. provider stream abort), Error, and Idle must NOT be stamped
            // Completed so queued prompts are never silently lost.
            let attempts = self.agent.accepted_attempts().unwrap_or(0);
            let completed = claimed_status.is_some_and(|status| {
                matches!(
                    status,
                    flowy_agent::TurnStatus::Completed
                        | flowy_agent::TurnStatus::DeliveryFailed
                )
            });
            match self.agent.finish_accepted(completed) {
                Ok(true) => {
                    self.emit_notice(Some("notice.receipt_gave_up"), cx);
                }
                Ok(false) if !completed && attempts == 3 => {
                    self.emit_notice(Some("notice.receipt_retrying"), cx);
                }
                Ok(false) => {}
                Err(error) => self.emit_notice(Some(error), cx),
            }
        }
        let can_drain = !self.status.is_busy()
            && !self.catalog_loading
            && (self.cloud_ready || !self.agent.requires_cloud_ready());
        if can_drain && !self.agent.pending_acceptances().is_empty() {
            let previous = self.agent.active_session_id();
            match self.agent.run_accepted_once() {
                Ok(Some(run)) => {
                    if self.surface == StudioSurface::Home && previous.is_none()
                    {
                        // Draining in the background from a blank Home stage
                        // must not bind the user's next Home prompt to the
                        // drained session.
                        let _ = self.agent.clear_active();
                    }
                    self.sync_receipts();
                    self.refresh_chat(cx);
                    if previous.as_deref() != Some(run.session_id.as_str()) {
                        self.emit_notice(Some("notice.accepted_ran"), cx);
                    }
                    return;
                }
                Ok(None) => {}
                Err(error) => self.emit_notice(Some(error), cx),
            }
        }
        self.sync_receipts();
        cx.notify();
    }

    pub fn open_session(&mut self, id: &str, cx: &mut Context<'_, Self>) {
        self.session_opened_by_user = true;
        // Reopening the current session must not round-trip its own draft.
        let same = self.agent.active_session_id().as_deref() == Some(id);
        let previous_session = if same {
            None
        } else {
            self.agent.active_session_id()
        };
        if let Some(current) = &previous_session {
            let draft = self.prompt_text(cx);
            self.drafts.insert(current.clone(), draft);
        }
        match self.agent.select_session(id) {
            Ok(()) => {
                if self.surface == StudioSurface::Board {
                    self.board_inspector.close_drawer();
                    self.drawer_preview.clear();
                }
                if !same {
                    self.sync_quote_session();
                    // Restore whatever this session had typed before.
                    let draft =
                        self.drafts.get(id).cloned().unwrap_or_default();
                    self.fill_prompt(&draft, cx);
                    self.clear_right_sheet();
                }
                self.invalidate_submission(cx);
                // The journal is global, but re-read it on every open so the
                // strip can never show a state older than the click that
                // brought the user here.
                self.sync_receipts();
                let workspace = self.agent.active_workspace();
                self.active_project_id =
                    workspace.as_ref().map(|value| value.project_id.clone());
                self.surface = if self.agent.has_messages() {
                    StudioSurface::Chat
                } else {
                    StudioSurface::Home
                };
                self.refresh_chat(cx);
                self.refresh_trace_cache();
                self.emit_notice(
                    workspace
                        .as_ref()
                        .filter(|value| {
                            !crate::projects::root_available(&value.root)
                        })
                        .map(|_| "project.folder_unavailable"),
                    cx,
                );
                cx.emit(StudioEvent::SessionsChanged);
                cx.notify();
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }

    /// Replace the file metadata projection without duplicating managed paths
    /// or reading disk on the UI thread.
    pub fn set_artifacts(
        &mut self,
        artifacts: Vec<crate::library::LibraryItem>,
        cx: &mut Context<'_, Self>,
    ) {
        self.feed
            .update(cx, |feed, cx| feed.set_artifacts(artifacts, cx));
        if self.right_drawer_open {
            self.refresh_drawer_metadata(cx);
            cx.notify();
        }
    }

    /// Explicit override bound to a specific session and real non-empty
    /// verification `report_id` (required by kernel `f8fb6b491`). Runs on a
    /// background worker with an in-flight guard so large SHA-256 hashes and
    /// `sync_all` never stall the GPUI frame or accept duplicate clicks.
    pub(crate) fn confirm_override_for_session(
        &mut self,
        session_id: &str,
        report_id: &str,
        artifact_path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if report_id.trim().is_empty() {
            self.emit_notice(Some("verify.override.missing_report"), cx);
            return;
        }
        if !self.overriding_reports.insert(report_id.to_owned()) {
            return;
        }
        cx.notify();
        let agent = self.agent.clone();
        let sid = session_id.to_owned();
        let rid = report_id.to_owned();
        let apath = artifact_path.to_owned();
        let guard_key = rid.clone();
        let job = cx.background_executor().spawn(async move {
            let result = agent.apply_card_action(CardAction::OverrideDeliver {
                session_id: sid,
                report_id: rid,
                artifact_path: apath,
            });
            let updates = agent.drain_updates();
            (result, updates)
        });
        cx.spawn(async move |weak, cx| {
            let (result, updates) = job.await;
            let _ = weak.update(cx, |panel, cx| {
                panel.overriding_reports.remove(guard_key.as_str());
                panel.process_agent_updates(updates, cx);
                match result {
                    Ok(_) => {
                        panel.board_inspector.override_ack_key = None;
                        panel.refresh_chat(cx);
                        panel.refresh_board_snapshot();
                        panel.emit_notice(None, cx);
                        cx.emit(StudioEvent::ArtifactsChanged);
                    }
                    Err(error) => panel.emit_notice(Some(error), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Resolve a durable artifact anchor only after the session projection is
    /// ready. The feed translates it to the visible virtual-list row.
    pub fn focus_artifact(
        &mut self,
        artifact_id: &str,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        self.feed
            .update(cx, |feed, cx| feed.focus_artifact(artifact_id, cx))
    }

    pub fn artifact_focus(
        &self,
        artifact_id: &str,
        cx: &gpui::App,
    ) -> Option<gpui::FocusHandle> {
        self.feed.read(cx).artifact_focus(artifact_id)
    }

    /// Refresh the projection after the host has deleted a conversation on a
    /// background worker. This method never mutates durable history itself.
    pub fn refresh_sessions_after_delete(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        let sessions = self.agent.list_sessions();
        self.draft_quotes
            .retain(|sid, _| sessions.iter().any(|s| s.id == *sid));
        if self.agent.active_session_id().is_none() {
            self.active_quote = None;
            self.quote_session = None;
            self.invalidate_submission(cx);
            self.clear_right_sheet();
            self.surface = StudioSurface::Home;
            self.document = DocumentSession::new(self.product);
            self.apply_draft_project(None);
            self.set_prompt_text("", cx);
        } else if let Some(crate::deliverables::InspectedWork::Session {
            id,
            ..
        }) = &self.inspected_work
        {
            let still_exists = sessions.iter().any(|session| {
                session.id == *id
                    || (self.agent.session_role(&session.id)
                        == flowy_agent::SessionRole::Router
                        && self
                            .agent
                            .child_works(&session.id)
                            .iter()
                            .any(|work| work.work_id == *id))
            });
            if !still_exists {
                self.clear_right_sheet();
            }
        }
        self.refresh_chat(cx);
        self.refresh_board_snapshot();
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
    }

    pub fn open_artifact_source(
        &mut self,
        session_id: &str,
        artifact_id: &str,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        let target_root = self.agent.root_session_id(session_id);
        if !self
            .agent
            .list_sessions()
            .iter()
            .any(|session| session.id == target_root)
        {
            return Err("library.source.unavailable");
        }
        let previous = self.agent.active_session_id();
        self.agent.select_session(&target_root)?;
        self.invalidate_submission(cx);
        self.refresh_chat(cx);
        if !self.focus_artifact(artifact_id, cx) {
            if let Some(previous) = previous {
                let _ = self.agent.select_session(&previous);
            } else {
                let _ = self.agent.clear_active();
            }
            self.refresh_chat(cx);
            return Err("library.source.unavailable");
        }
        self.active_project_id = self
            .agent
            .active_workspace()
            .map(|workspace| workspace.project_id);
        self.session_opened_by_user = true;
        self.surface = StudioSurface::Chat;
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.refresh_trace_cache();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    /// Render and focus the session that the host has already selected and
    /// verified on a worker. This method only reads the in-memory projection.
    pub fn show_selected_artifact(
        &mut self,
        artifact_id: &str,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        self.invalidate_submission(cx);
        self.refresh_chat(cx);
        if !self.focus_artifact(artifact_id, cx) {
            return Err("library.source.unavailable");
        }
        self.active_project_id = self
            .agent
            .active_workspace()
            .map(|workspace| workspace.project_id);
        self.session_opened_by_user = true;
        self.surface = StudioSurface::Chat;
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.refresh_trace_cache();
        self.emit_notice(None, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    /// Whether the user opened a conversation in this run. The app shell uses
    /// this to keep launch on Home while still restoring an opened session.
    pub const fn session_opened_by_user(&self) -> bool {
        self.session_opened_by_user
    }

    pub fn create_project(
        &mut self,
        name: &str,
        folder: &std::path::Path,
        cx: &mut Context<'_, Self>,
    ) -> Result<crate::projects::Project, &'static str> {
        if self.project_store_failed {
            return Err("project.storage_error");
        }
        let project = crate::projects::create(
            &self.store_root,
            &mut self.projects,
            name,
            folder,
        )
        .map_err(|error| error.notice_key())?;
        self.open_project(&project.id, cx)?;
        self.refresh_board_snapshot();
        self.refresh_local_env();
        Ok(project)
    }

    pub fn update_project(
        &mut self,
        id: &str,
        name: &str,
        folder: &std::path::Path,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        if self.project_store_failed {
            return Err("project.storage_error");
        }
        let project = crate::projects::update(
            &self.store_root,
            &mut self.projects,
            id,
            name,
            folder,
        )
        .map_err(|error| error.notice_key())?;
        if self.active_project_id.as_deref() == Some(id) {
            self.agent
                .set_workspace_for_new_sessions(Some(AgentWorkspace {
                    project_id: project.id,
                    root: project.root,
                }));
        }
        self.refresh_board_snapshot();
        self.refresh_local_env();
        cx.notify();
        Ok(())
    }

    pub fn remove_project(
        &mut self,
        id: &str,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        if self.project_store_failed {
            return Err("project.storage_error");
        }
        let was_active = self.active_project_id.as_deref() == Some(id);
        crate::projects::remove(&self.store_root, &mut self.projects, id)
            .map_err(|error| error.notice_key())?;
        if self.board_project_filter.as_deref() == Some(id) {
            self.board_project_filter = None;
        }
        if was_active {
            if let Some(sid) = self.agent.active_session_id() {
                self.draft_quotes.remove(&sid);
            }
            self.active_quote = None;
            self.quote_session = None;
            self.active_project_id = None;
            let _ = self.agent.clear_active();
            self.agent.set_workspace_for_new_sessions(None);
            self.surface = StudioSurface::Home;
            self.board_inspector.close_drawer();
            self.clear_right_sheet();
            self.refresh_chat(cx);
            self.emit_notice(None, cx);
        }
        self.refresh_board_snapshot();
        self.refresh_local_env();
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    pub fn set_project_pinned(
        &mut self,
        id: &str,
        pinned: bool,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        if self.project_store_failed {
            return Err("project.storage_error");
        }
        crate::projects::set_pinned(
            &self.store_root,
            &mut self.projects,
            id,
            pinned,
        )
        .map_err(|error| error.notice_key())?;
        cx.notify();
        Ok(())
    }

    pub fn view_project(&self, id: &str) -> Result<(), &'static str> {
        let project = self
            .projects
            .iter()
            .find(|project| project.id == id)
            .ok_or("project.folder_unavailable")?;
        if !crate::projects::root_available(&project.root) {
            return Err("project.folder_unavailable");
        }
        let safe_dir = crate::ports::resolve_safe_local_dir(&project.root)
            .ok_or("project.folder_unavailable")?;
        open::that(&safe_dir).map_err(|_| "project.view_failed")
    }

    pub fn open_project(
        &mut self,
        id: &str,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), &'static str> {
        let Some(project) = self
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
        else {
            return Err("project.folder_unavailable");
        };
        if !crate::projects::root_available(&project.root) {
            self.emit_notice(Some("project.folder_unavailable"), cx);
            return Err("project.folder_unavailable");
        }
        let clear_error = self.agent.clear_active().err();
        self.invalidate_submission(cx);
        self.board_inspector.close_drawer();
        self.clear_right_sheet();
        self.apply_draft_project(Some(&project));
        self.surface = StudioSurface::Home;
        self.set_prompt_text("", cx);
        self.refresh_chat(cx);
        self.refresh_popover_works();
        self.refresh_local_env();
        self.emit_notice(clear_error, cx);
        cx.emit(StudioEvent::SessionsChanged);
        cx.notify();
        Ok(())
    }

    pub fn open_thread(&mut self, title: String, cx: &mut Context<'_, Self>) {
        self.set_prompt_text(title, cx);
        self.send_prompt(cx);
    }

    pub fn clear_thread(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(sid) = self.agent.active_session_id() {
            self.draft_quotes.remove(&sid);
        }
        self.active_quote = None;
        self.quote_session = None;
        self.new_task(cx);
    }

    pub fn new_conversation(&mut self, cx: &mut Context<'_, Self>) {
        self.new_task(cx);
    }
}
