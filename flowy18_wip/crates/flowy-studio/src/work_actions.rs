//! Visual Board, Flowchart Modal, local environment, popover reset, and
//! decoupled task control actions (`cancel_task` / `apply_card_action`) on
//! `StudioPanel`.

use flowy_agent::{
    CancelOutcome, CardAction, CardActionOutcome, ReceiptState, TaskTarget,
};
use gpui::Context;

use crate::panel::{StudioPanel, StudioSurface};

impl StudioPanel {
    /// Synchronizes both claimable pending receipts (for legacy tests/counts)
    /// and all non-terminal visible receipts (`Queued`/`Running`/`Retrying`/
    /// `Quarantined`/`Dead`) from the kernel's durable journal.
    pub(crate) fn sync_receipts(&mut self) {
        self.pending_receipts = self.agent.pending_acceptances();
        self.visible_receipts = self.agent.visible_acceptances();
    }

    /// Whether `path` is an existing regular file allowed to launch in the OS
    /// default application (blocks executables/scripts like `.exe`/`.ps1`).
    #[must_use]
    pub(crate) fn can_open_file_externally(path: &str) -> bool {
        flowy_agent::can_open_with_default_app(path)
            && std::path::Path::new(path).is_file()
    }

    /// Opens `path` in the OS default application after verifying both the
    /// safe-extension allowlist and that `path` points to a regular file.
    pub(crate) fn open_file_externally(
        &mut self,
        path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        if !Self::can_open_file_externally(path) || open::that(path).is_err() {
            self.emit_notice(Some("library.error.open"), cx);
        }
    }

    /// Clears cloud credentials, cancels any in-flight turns across all
    /// sessions via `TaskTarget::Session`, applies the disconnected config,
    /// and resets UI v2 overlays, filters, and retry state on sign-out.
    pub fn prepare_for_sign_out(
        &mut self,
        disconnected: flowy_agent::LlmConfig,
        cx: &mut Context<'_, Self>,
    ) {
        self.cloud_ready = false;
        self.catalog_loading = false;
        self.catalog = flowy_agent::ProviderCatalog::new();
        self.tiers.clear();
        self.plan = flowy_agent::Plan::Free;
        self.credits = crate::CreditsViewState::Unavailable;
        self.selected_skill = None;
        self.board_project_filter = None;
        self.retrying_receipt = None;
        self.retrying_delivery = None;
        self.close_all_popovers();
        self.flow_modal_session = None;
        self.flow_modal_branch = None;
        self.flow_modal_override_ack = false;
        self.clear_right_sheet();
        let mut busy = self.agent.busy_session_ids();
        if busy.is_empty()
            && let Some(active) = self.agent.active_session_id()
        {
            busy.push(active);
        }
        for session_id in busy {
            let _ = self.agent.cancel_task(TaskTarget::Session(session_id));
        }
        let _ = self.agent.apply_config(disconnected);
        self.sync_receipts();
        self.refresh_local_env();
        self.refresh_board_snapshot();
        cx.notify();
    }

    /// Closes all mutually exclusive transient popovers in one place so new
    /// cockpit menus cannot be missed by navigation or dialog transitions.
    pub fn close_all_popovers(&mut self) {
        self.model_info_open = false;
        self.model_menu_open = false;
        self.bg_tasks_open = false;
        self.env_popover_open = false;
        self.workspace_menu_open = false;
        self.skill_menu_open = false;
        self.credits_hover = false;
        self.credits_close = None;
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

    /// Requests a graceful stop for one specific background or active session
    /// without coupling cancellation to whichever session is currently focused.
    pub(crate) fn stop_session_turn(
        &self,
        session_id: &str,
        cx: &Context<'_, Self>,
    ) {
        self.cancel_task_target(TaskTarget::Session(session_id.to_owned()), cx);
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
        if self.retrying_receipt.as_deref() == Some(receipt_id) {
            return;
        }
        self.retrying_receipt = Some(receipt_id.to_owned());
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
                if panel.retrying_receipt.as_deref() == Some(target_id.as_str())
                {
                    panel.retrying_receipt = None;
                }
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

    /// Rebuilds the cached Visual Board snapshot from `AgentHandle`.
    /// Called strictly on structural `AgentUpdate`, project changes, or opening
    /// the Board — never per-token or inside `Render::render`.
    pub(crate) fn refresh_board_snapshot(&mut self) {
        use crate::ports::BoardDataSource as _;
        self.board_generation = self.board_generation.wrapping_add(1);
        let default_label = self.t("studio.space_default_sub");
        self.board_snapshot = self.agent.build_board_snapshot(
            &self.projects,
            &self.visible_receipts,
            &default_label,
            self.board_generation,
        );
        if let Some(open_sid) = self.flow_modal_session.as_deref()
            && !self
                .board_snapshot
                .sessions
                .iter()
                .any(|group| group.session_id == open_sid)
        {
            self.flow_modal_session = None;
            self.flow_modal_branch = None;
            self.flow_modal_override_ack = false;
        }
        self.last_board_refresh = Some(std::time::Instant::now());
    }

    /// Rebuilds the cached local environment health snapshot (never called per
    /// render frame).
    pub(crate) fn refresh_local_env(&mut self) {
        self.local_env = crate::ports::probe_local_env(
            &self.projects,
            &self.installed_skills,
            self.cloud_ready,
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
        self.flow_modal_session = None;
        self.clear_right_sheet();
        self.close_all_popovers();
        self.surface = StudioSurface::Board;
        self.refresh_board_snapshot();
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
        cx.notify();
    }

    /// Opens the centered Flowchart Modal for `session_id`.
    pub(crate) fn open_flow_modal(
        &mut self,
        session_id: String,
        branch_id: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.flow_modal_session = Some(session_id);
        self.flow_modal_branch = branch_id;
        self.flow_modal_tab = crate::overview::FlowModalTab::Action;
        self.flow_modal_override_ack = false;
        cx.notify();
    }

    /// Closes the centered Flowchart Modal cleanly.
    pub fn close_flow_modal(&mut self, cx: &mut Context<'_, Self>) {
        self.flow_modal_session = None;
        self.flow_modal_branch = None;
        self.flow_modal_override_ack = false;
        cx.notify();
    }

    /// Whether the centered Flowchart Modal is currently visible.
    #[must_use]
    pub const fn has_flow_modal(&self) -> bool {
        self.flow_modal_session.is_some()
    }

    /// Jumps straight into the Flowchart Modal for the first session waiting on
    /// user approval or dead/quarantined receipt recovery.
    pub(crate) fn open_first_blocker_modal(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) {
        let found = self.board_snapshot.sessions.iter().find_map(|group| {
            group
                .branches
                .iter()
                .find(|branch| {
                    matches!(
                        branch.phase,
                        crate::ports::BranchPhase::NeedsApproval { .. }
                            | crate::ports::BranchPhase::Queued {
                                state: ReceiptState::Dead
                                    | ReceiptState::Quarantined,
                                ..
                            }
                    )
                })
                .map(|branch| (group.session_id.clone(), branch.id.clone()))
        });
        if let Some((sid, bid)) = found {
            self.open_flow_modal(sid, Some(bid), cx);
        } else {
            self.emit_notice(Some("board.no_blockers"), cx);
        }
    }

    /// Pushes installed skills from `flowy-app` (`flowy-skills`) into Studio.
    pub fn set_installed_skills(
        &mut self,
        skills: Vec<crate::ports::SkillChip>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.installed_skills != skills {
            self.installed_skills = skills;
            self.refresh_local_env();
            cx.notify();
        }
    }

    /// Cached count of branches waiting for explicit user approval.
    #[must_use]
    pub const fn board_blocker_count(&self) -> usize {
        self.board_snapshot.kpi.approval_count
    }

    /// Cached count of active (waiting approval + running) branches for dock badge.
    #[must_use]
    pub const fn board_active_count(&self) -> usize {
        self.board_snapshot.kpi.approval_count
            + self.board_snapshot.kpi.running_count
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

    /// Opens the parent directory holding `path` in the OS file manager.
    pub(crate) fn reveal_file_in_os(
        &mut self,
        path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.reveal_file(path, cx);
    }

    /// Registers the parent directory of a delivered file as a persistent local
    /// project in the sidebar (without modifying kernel workspace paths).
    pub(crate) fn register_deliverable_folder_as_project(
        &mut self,
        file_path: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(parent) = std::path::Path::new(file_path).parent() else {
            self.emit_notice(Some("project.invalid_folder"), cx);
            return;
        };
        let name = parent
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Flowy")
            .to_owned();
        match self.create_project(&name, parent, cx) {
            Ok(_) => {
                self.refresh_board_snapshot();
                self.emit_notice(Some("deliverables.promoted_project"), cx);
            }
            Err(key) => self.emit_notice(Some(key), cx),
        }
    }
}
