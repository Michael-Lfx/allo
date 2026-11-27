use super::{StudioEvent, StudioPanel, StudioSurface};
use flowy_agent::{AgentWorkspace, SessionOrigin, TaskTarget};
use gpui::{Context, Window};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn new_submission_id() -> u128 {
    static NEXT: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (timestamp << 32) | u128::from(std::process::id() ^ (sequence as u32))
}

/// Formats an outgoing prompt using `flowy-agent`'s real `/skill:<owner>/<slug>`
/// invocation protocol when an installed skill chip is mounted.
pub(super) fn format_skill_prompt(
    selected: Option<&crate::ports::SkillChip>,
    trimmed: &str,
) -> String {
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(skill) = selected
        && !trimmed.starts_with("/skill:")
    {
        return format!("/skill:{} {trimmed}", skill.id);
    }
    trimmed.to_owned()
}

impl StudioPanel {
    pub(super) fn sync_quote_session(&mut self) {
        let current = self.agent.active_session_id();
        if current != self.quote_session {
            let previous = self.quote_session.take();
            self.stash_active_quote(previous);
            self.active_quote = current
                .as_deref()
                .and_then(|id| self.draft_quotes.remove(id));
            self.quote_session = current;
        }
    }

    pub(crate) fn invalidate_submission(&mut self, cx: &mut Context<'_, Self>) {
        self.submission_generation = self.submission_generation.wrapping_add(1);
        self.submission_pending = false;
        self.submission_stop = None;
        self.submission = None;
        self.refresh_working(cx);
    }

    /// Whether the composer should allow submitting immediately (or queueing
    /// during cold-start catalog loading) rather than showing the model-setup
    /// amber warning state.
    #[must_use]
    pub(crate) fn can_submit_prompt(&self) -> bool {
        self.cloud_ready
            || self.catalog_loading
            || !self.agent.requires_cloud_ready()
    }

    /// Binds (or clears) the pending draft project workspace and origin for
    /// lazily created Router sessions (Q1).
    pub(crate) fn apply_draft_project(
        &mut self,
        project: Option<&crate::projects::Project>,
    ) {
        if let Some(project) = project {
            let project_id = project.id.clone();
            self.active_project_id = Some(project_id.clone());
            self.agent
                .set_workspace_for_new_sessions(Some(AgentWorkspace {
                    project_id: project.id.clone(),
                    root: project.root.clone(),
                }));
            self.agent
                .set_origin_for_new_sessions(SessionOrigin::Project {
                    project_id: Some(project_id),
                });
        } else {
            self.active_project_id = None;
            self.agent.set_workspace_for_new_sessions(None);
            self.agent.set_origin_for_new_sessions(SessionOrigin::Home);
        }
    }

    pub(crate) fn set_active_quote(
        &mut self,
        role: crate::composer::QuoteRole,
        raw_text: &str,
        cx: &mut Context<'_, Self>,
    ) {
        let excerpt = crate::composer::make_quote_excerpt(raw_text);
        if excerpt.is_empty() {
            return;
        }
        self.active_quote =
            Some(crate::composer::ActiveQuote { role, excerpt });
        self.quote_session = self.agent.active_session_id();
        cx.notify();
    }

    pub(super) fn stash_active_quote(&mut self, session_id: Option<String>) {
        if let Some(id) = session_id {
            if let Some(quote) = self.active_quote.take() {
                self.draft_quotes.insert(id, quote);
            } else {
                self.draft_quotes.remove(&id);
            }
        } else {
            self.active_quote = None;
        }
    }

    fn draft_workspace(&self) -> Option<AgentWorkspace> {
        if let Some(pid) = self.active_project_id.as_deref() {
            self.projects
                .iter()
                .find(|p| p.id == pid)
                .map(|p| AgentWorkspace {
                    project_id: p.id.clone(),
                    root: p.root.clone(),
                })
        } else {
            self.agent.active_workspace()
        }
    }

    pub fn send_prompt(&mut self, cx: &mut Context<'_, Self>) {
        if self.submission_pending {
            return;
        }
        if self.agent.is_initializing() {
            self.emit_notice(Some("library.loading"), cx);
            return;
        }
        let model = self.catalog.selected_model().map(|m| m.id.clone());
        // Text queued for the next render has not reached the prompt state yet;
        // it is still what the user asked to send.
        let typed = self.prompt_text(cx);
        let trimmed = typed.trim();
        let trimmed_draft = trimmed.to_owned();
        let after_skill =
            format_skill_prompt(self.selected_skill.as_ref(), trimmed);
        let submitted_quote = self.active_quote.clone();
        let text = crate::composer::compose_quoted_prompt(
            submitted_quote.as_ref(),
            &after_skill,
        );
        tracing::debug!(
            target: "flowy.prompt",
            stage = "StudioPanel::send_prompt",
            cloud_ready = self.cloud_ready,
            model = ?model,
            chars = text.chars().count(),
            "enter"
        );
        if after_skill.is_empty() {
            tracing::debug!(
                target: "flowy.prompt",
                stage = "StudioPanel::send_prompt",
                reason = "empty",
                "reject"
            );
            self.emit_notice(Some("notice.prompt_empty"), cx);
            return;
        }
        let active_session = self.agent.active_session_id();
        let active_role = active_session
            .as_deref()
            .map(|sid| self.agent.session_role(sid));
        let is_legacy_standard =
            active_role == Some(flowy_agent::SessionRole::Standard);
        let use_router_path = !is_legacy_standard;

        // Only pause sending when the active conversation ITSELF has a pending
        // card (Router's own disambiguation card or Standard's card). Child
        // Worker approval cards never block the main Router conversation (Q4).
        let has_own_pending_card =
            matches!(self.status, flowy_agent::TurnStatus::WaitingForUser)
                || active_session.as_deref().is_some_and(|sid| {
                    !self.agent.session_pending_cards(sid).is_empty()
                });
        if has_own_pending_card {
            self.emit_notice(Some("notice.card_pending"), cx);
            return;
        }
        let running = self.status.is_busy();
        let waiting_for_cloud = self.catalog_loading
            || ((!self.cloud_ready || model.is_none())
                && self.agent.requires_cloud_ready());
        // Legacy Standard sessions defer whenever running or awaiting cloud
        // startup; fresh drafts defer whenever awaiting cloud startup; existing
        // Router sessions defer only during cold-start catalog load (warm busy
        // Routers continue through `pending_router_followups`).
        let should_defer = if is_legacy_standard {
            running || waiting_for_cloud
        } else {
            waiting_for_cloud
                && (active_session.is_none() || self.catalog_loading)
        };
        if should_defer {
            let draft_workspace = self.draft_workspace();
            let accepted = if is_legacy_standard {
                self.agent.accept_deferred(&text)
            } else if let Some(router_id) = active_session.as_deref() {
                self.agent.accept_deferred_router(router_id, &text)
            } else {
                let draft_origin = match &draft_workspace {
                    Some(ws) => flowy_agent::SessionOrigin::Project {
                        project_id: Some(ws.project_id.clone()),
                    },
                    None => flowy_agent::SessionOrigin::Home,
                };
                self.agent.accept_deferred_draft(
                    &text,
                    draft_origin,
                    draft_workspace,
                    flowy_agent::SessionRole::Router,
                )
            };
            match accepted {
                Ok(receipt) => {
                    tracing::debug!(
                        target: "flowy.prompt",
                        stage = "StudioPanel::send_prompt",
                        receipt = %receipt.id,
                        legacy = is_legacy_standard,
                        "accepted-deferred"
                    );
                    self.set_prompt_text("", cx);
                    self.active_quote = None;
                    if let Some(sid) = active_session.as_deref() {
                        self.draft_quotes.remove(sid);
                    }
                    if self.agent.autonomous_drain() {
                        self.sync_receipts();
                        self.refresh_board_snapshot();
                        cx.notify();
                    } else {
                        self.refresh_pending_receipts(cx);
                    }
                }
                Err(error) => self.emit_notice(Some(error), cx),
            }
            return;
        }
        if !self.cloud_ready || model.is_none() {
            tracing::debug!(
                target: "flowy.prompt",
                stage = "StudioPanel::send_prompt",
                reason = "cloud_or_model",
                "reject"
            );
            self.emit_notice(Some("notice.agent_failed"), cx);
            return;
        }
        let id = *self.submission.get_or_insert_with(new_submission_id);
        self.submission_generation = self.submission_generation.wrapping_add(1);
        let generation = self.submission_generation;
        self.submission_pending = true;
        let stop = Arc::new(AtomicBool::new(false));
        self.submission_stop = Some(stop.clone());
        self.refresh_working(cx);
        self.refresh_popover_works();
        cx.notify();
        let agent = self.agent.clone();
        let expected_generation = agent.context_generation();
        let expected_active = active_session;
        let expected_workspace = self.draft_workspace();
        let submitted_text = text.clone();
        let expected = expected_active.clone();
        let worker_stop = stop.clone();
        let worker_agent = agent.clone();
        let job = cx.background_executor().spawn(async move {
            let result = if worker_stop.load(Ordering::Acquire) {
                Err("notice.agent_busy")
            } else if expected_workspace.as_ref().is_some_and(|workspace| {
                !crate::projects::root_available(&workspace.root)
            }) {
                Err("project.folder_unavailable")
            } else if use_router_path {
                worker_agent.submit_router_chat_if_active(
                    id,
                    &submitted_text,
                    expected.as_deref(),
                    expected_generation,
                    expected_workspace,
                )
            } else {
                worker_agent.submit_chat_if_active(
                    id,
                    &submitted_text,
                    expected.as_deref(),
                    expected_generation,
                )
            };
            let stopped = if let Ok(session_id) = &result
                && worker_stop.load(Ordering::Acquire)
            {
                if use_router_path {
                    let _ = worker_agent.stop_router_reply(session_id);
                } else {
                    let _ = worker_agent
                        .cancel_task(TaskTarget::Session(session_id.clone()));
                }
                true
            } else {
                false
            };
            (result, stopped)
        });
        cx.spawn(async move |weak, cx| {
            let (result, stopped) = job.await;
            // A Stop click can arrive after the worker returns its receipt but
            // before this UI continuation runs. Keep it bound to that receipt.
            if !stopped
                && stop.load(Ordering::Acquire)
                && let Ok(session_id) = &result
            {
                let session_id = session_id.clone();
                cx.background_executor()
                    .spawn(async move {
                        if use_router_path {
                            let _ = agent.stop_router_reply(&session_id);
                        } else {
                            let _ = agent
                                .cancel_task(TaskTarget::Session(session_id));
                        }
                    })
                    .detach();
            }
            let _ = weak.update(cx, |panel, cx| {
                if generation != panel.submission_generation
                    || !panel.submission_pending
                    || panel.submission != Some(id)
                {
                    return;
                }
                panel.submission_pending = false;
                panel.submission_stop = None;
                let active = panel.agent.active_session_id();
                let draft_matches =
                    panel.prompt_text(cx).trim() == trimmed_draft;
                match result {
                    Ok(session_id)
                        if active.as_deref() == Some(session_id.as_str()) =>
                    {
                        panel.submission = None;
                        if draft_matches {
                            panel.set_prompt_text("", cx);
                        }
                        if panel.active_quote == submitted_quote {
                            panel.active_quote = None;
                        }
                        if submitted_quote.is_some()
                            && panel.draft_quotes.get(&session_id)
                                == submitted_quote.as_ref()
                        {
                            panel.draft_quotes.remove(&session_id);
                        }
                        panel.session_opened_by_user = true;
                        panel.surface = StudioSurface::Chat;
                        panel.refresh_chat(cx);
                        panel.refresh_board_snapshot();
                        panel.emit_notice(None, cx);
                        cx.emit(StudioEvent::SessionsChanged);
                    }
                    Ok(session_id) => {
                        // User switched context mid-send: keep the new context's
                        // draft untouched while refreshing sessions/board.
                        panel.submission = None;
                        if submitted_quote.is_some()
                            && panel.draft_quotes.get(&session_id)
                                == submitted_quote.as_ref()
                        {
                            panel.draft_quotes.remove(&session_id);
                        }
                        panel.refresh_board_snapshot();
                        cx.emit(StudioEvent::SessionsChanged);
                    }
                    Err(key)
                        if active == expected_active
                            || expected_active.is_none() =>
                    {
                        if !draft_matches {
                            panel.submission = None;
                        }
                        panel.emit_notice(Some(key), cx);
                    }
                    _ => panel.submission = None,
                }
                panel.refresh_working(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn retry_delivery(&mut self, cx: &mut Context<'_, Self>) {
        if !matches!(self.status, flowy_agent::TurnStatus::DeliveryFailed) {
            return;
        }
        let Some(expected) = self.agent.active_session_id() else {
            return;
        };
        self.retry_delivery_for_session(&expected, cx);
    }

    pub fn stop_turn(&mut self, cx: &mut Context<'_, Self>) {
        if self.submission_pending {
            if let Some(stop) = &self.submission_stop {
                stop.store(true, Ordering::Release);
            }
            cx.notify();
        } else if let Some(session_id) = self.agent.active_session_id() {
            self.stop_session_turn(&session_id, cx);
        }
    }

    pub fn fill_prompt(&mut self, text: &str, cx: &mut Context<'_, Self>) {
        self.set_prompt_text(text, cx);
    }

    /// The prompt text the user sees, including text queued for the next render.
    pub(super) fn prompt_text(&self, cx: &Context<'_, Self>) -> String {
        match self.prompt_pending.as_deref() {
            Some(pending) => pending.to_owned(),
            None => self.prompt.read(cx).value().to_string(),
        }
    }

    /// Queue prompt text for the render pass.
    ///
    /// The editing state installs programmatic text through `set_value`, which
    /// needs a `&mut Window`; most prompt entry points (session restore,
    /// background submission callbacks) run without one, so the text is applied
    /// at the top of `render`.
    pub(super) fn set_prompt_text(
        &mut self,
        text: impl Into<String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.prompt_pending = Some(text.into());
        cx.notify();
    }

    /// Install queued prompt text once a window is available.
    pub(super) fn apply_prompt_pending(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(text) = self.prompt_pending.take() else {
            return;
        };
        self.prompt.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            // `set_value` resets a multi-line selection to the start; a
            // programmatic fill leaves the caret where typing continues.
            let end = state.value().len();
            state.set_selected_range(end..end, cx);
        });
    }

    pub fn fill_prompt_key(
        &mut self,
        key: &'static str,
        cx: &mut Context<'_, Self>,
    ) {
        let text = self.t(key);
        self.fill_prompt(&text, cx);
    }

    /// Fills the prompt from a starter. Picking one means "show me this
    /// prompt", so an existing draft is replaced without asking.
    pub(crate) fn request_fill_prompt(
        &mut self,
        text: String,
        cx: &mut Context<'_, Self>,
    ) {
        self.fill_prompt(&text, cx);
    }
}
