//! Event-driven cached Router active child-Works status pill, queued follow-up
//! badge, and two-section (`This conversation` / `Other conversations`)
//! background popover (PR-2B Q2 / Q5 / Q6).

use std::collections::{HashMap, HashSet};

use flowy_agent::{
    AcceptedMessage, ChildWorkSummary, ReceiptState, SessionRole,
};
use flowy_ui as theme;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div,
    prelude::*, px,
};

use crate::panel::{StudioPanel, StudioSurface};

/// Visual tone for the compact `bg-tasks-slot` and Home resume pills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BgPillTone {
    Active,
    Attention,
}

/// Compact summary rendered in `chat.rs::bg_tasks_dock_slot` and `home.rs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BgPillSummary {
    pub label: String,
    pub tone: BgPillTone,
    pub count: usize,
    pub detail: Option<String>,
}

/// Cached active-work snapshot for the two-section popover (deduplicated by `work_id`).
#[derive(Clone, Debug, Default)]
pub struct PopoverWorksSnapshot {
    pub current_works: Vec<ChildWorkSummary>,
    pub other_works: Vec<(String, ChildWorkSummary)>,
    pub legacy_running_sessions: Vec<(String, String)>,
    pub legacy_receipts: Vec<(AcceptedMessage, ReceiptState)>,
}

impl PopoverWorksSnapshot {
    #[must_use]
    pub const fn total_count(&self) -> usize {
        self.current_works.len()
            + self.other_works.len()
            + self.legacy_running_sessions.len()
            + self.legacy_receipts.len()
    }
}

fn is_work_in_flight(state: &str) -> bool {
    matches!(
        state,
        "awaiting_approval" | "running" | "verifying" | "queued" | "stopping"
    )
}

fn is_work_attention(state: &str) -> bool {
    matches!(state, "awaiting_approval" | "failed" | "partial")
}

fn is_work_cancellable(state: &str) -> bool {
    matches!(
        state,
        "awaiting_approval" | "running" | "verifying" | "queued"
    )
}

fn work_priority_rank(state: &str) -> u8 {
    match state {
        "awaiting_approval" => 0,
        "failed" | "partial" => 1,
        "running" | "verifying" => 2,
        "stopping" => 3,
        "queued" => 4,
        _ => 5,
    }
}

impl StudioPanel {
    /// Rebuilds the cached popover/pill snapshot from `AgentHandle`.
    /// Called strictly on structural updates, surface changes, or throttled
    /// background-worker updates — never inside `Render::render`.
    pub(crate) fn refresh_popover_works(&mut self) {
        self.popover_works = self.collect_popover_works();
        self.last_popover_works_refresh = Some(std::time::Instant::now());
    }

    /// Collect deduplicated active/attention child works for the current Router
    /// conversation and other top-level conversations, plus any legacy/other
    /// running sessions and non-Worker receipts.
    pub(crate) fn collect_popover_works(&self) -> PopoverWorksSnapshot {
        let active_id = self.agent.active_session_id();
        let active_is_router = active_id.as_deref().is_some_and(|sid| {
            self.agent.session_role(sid) == SessionRole::Router
        });
        let sessions = self.agent.list_sessions();
        let titles: HashMap<&str, &str> = sessions
            .iter()
            .map(|s| (s.id.as_str(), s.title.as_str()))
            .collect();

        let mut seen_work_ids: HashSet<String> = HashSet::new();
        let mut current_works: Vec<ChildWorkSummary> = Vec::new();
        let mut other_works: Vec<(String, ChildWorkSummary)> = Vec::new();

        if let Some(router_id) = active_id.as_deref()
            && active_is_router
        {
            for work in self.agent.child_works(router_id) {
                if (is_work_in_flight(&work.state)
                    || is_work_attention(&work.state))
                    && seen_work_ids.insert(work.work_id.clone())
                {
                    current_works.push(work);
                }
            }
            current_works.sort_by_key(|w| {
                (
                    work_priority_rank(&w.state),
                    std::cmp::Reverse(w.updated_at),
                )
            });
        }

        for s in &sessions {
            if s.archived
                || active_id.as_deref() == Some(s.id.as_str())
                || self.agent.session_role(&s.id) != SessionRole::Router
            {
                continue;
            }
            let parent_title = if s.title.trim().is_empty() {
                crate::ports::short_id(&s.id)
            } else {
                crate::ports::redact_trace_text(&s.title)
            };
            for work in self.agent.child_works(&s.id) {
                if (is_work_in_flight(&work.state)
                    || is_work_attention(&work.state))
                    && seen_work_ids.insert(work.work_id.clone())
                {
                    other_works.push((parent_title.clone(), work));
                }
            }
        }
        other_works.sort_by_key(|(_, w)| {
            (
                work_priority_rank(&w.state),
                std::cmp::Reverse(w.updated_at),
            )
        });

        let running_ids = self.background_running_session_ids();
        let mut legacy_running_sessions = Vec::new();
        for sid in &running_ids {
            if seen_work_ids.contains(sid)
                || self.agent.session_role(sid) == SessionRole::Worker
            {
                continue;
            }
            let title = titles
                .get(sid.as_str())
                .copied()
                .filter(|t| !t.trim().is_empty())
                .map(crate::ports::redact_trace_text)
                .or_else(|| {
                    self.agent
                        .session_preview(sid)
                        .map(|t| crate::ports::redact_trace_text(&t))
                })
                .unwrap_or_else(|| crate::ports::short_id(sid));
            legacy_running_sessions.push((sid.clone(), title));
        }

        let legacy_receipts: Vec<(AcceptedMessage, ReceiptState)> = self
            .popover_visible_receipts(&running_ids)
            .into_iter()
            .filter(|(receipt, _)| {
                let target = receipt
                    .target_session_id
                    .as_deref()
                    .or(receipt.session_hint.as_deref());
                !target.is_some_and(|sid| {
                    seen_work_ids.contains(sid)
                        || self.agent.session_role(sid) == SessionRole::Worker
                })
            })
            .map(|(r, s)| (r.clone(), s))
            .collect();

        PopoverWorksSnapshot {
            current_works,
            other_works,
            legacy_running_sessions,
            legacy_receipts,
        }
    }

    #[must_use]
    pub(crate) fn show_anonymous_active_work(&self) -> bool {
        self.surface != StudioSurface::Chat
            && self.popover_works.total_count() == 0
            && (self.status.is_busy() || self.submission_pending)
    }

    /// Single source of truth for background work count across Chat pill, Home
    /// resume pill, and the `bg-tasks-panel` header badge.
    #[must_use]
    pub(crate) fn popover_total_count(&self) -> usize {
        self.popover_works.total_count()
            + usize::from(self.show_anonymous_active_work())
    }

    fn summarize_works_slice(
        &self,
        works: &[&ChildWorkSummary],
        total_count: usize,
        include_detail: bool,
    ) -> BgPillSummary {
        let approval_count = works
            .iter()
            .filter(|w| w.state == "awaiting_approval")
            .count();
        let failed_count = works
            .iter()
            .filter(|w| matches!(w.state.as_str(), "failed" | "partial"))
            .count();
        let detail = include_detail
            .then(|| {
                works.first().and_then(|top| {
                    top.heartbeat.as_ref().and_then(|hb| {
                        crate::pending_card::format_heartbeat_line_localized(
                            hb,
                            &|key| self.t(key),
                        )
                    })
                })
            })
            .flatten();
        if approval_count > 0 {
            return BgPillSummary {
                label: self.tf(
                    "chat.bg_tasks_needs_approval",
                    &[("count", &approval_count.to_string())],
                ),
                tone: BgPillTone::Attention,
                count: total_count,
                detail,
            };
        }
        if failed_count > 0 {
            return BgPillSummary {
                label: self.tf(
                    "chat.bg_tasks_failed",
                    &[("count", &failed_count.to_string())],
                ),
                tone: BgPillTone::Attention,
                count: total_count,
                detail,
            };
        }
        BgPillSummary {
            label: self.tf(
                "chat.bg_tasks_pill",
                &[("count", &total_count.to_string())],
            ),
            tone: BgPillTone::Active,
            count: total_count,
            detail,
        }
    }

    /// O(1) read of the priority-ordered `bg-tasks-slot` / Home pill summary (Q2):
    /// shows the current conversation's count + most urgent status
    /// (`awaiting_approval` > `failed/partial` > active), falling back to global
    /// background works (preserving urgent attention tone on Home).
    #[must_use]
    pub(crate) fn router_bg_pill_summary(&self) -> Option<BgPillSummary> {
        let snap = &self.popover_works;
        if !snap.current_works.is_empty() {
            let refs: Vec<&ChildWorkSummary> =
                snap.current_works.iter().collect();
            return Some(self.summarize_works_slice(
                &refs,
                snap.current_works.len(),
                true,
            ));
        }

        let total = self.popover_total_count();
        if total == 0 {
            return None;
        }
        if !snap.other_works.is_empty() {
            let refs: Vec<&ChildWorkSummary> =
                snap.other_works.iter().map(|(_, w)| w).collect();
            return Some(self.summarize_works_slice(&refs, total, false));
        }
        Some(BgPillSummary {
            label: self
                .tf("chat.bg_tasks_pill", &[("count", &total.to_string())]),
            tone: BgPillTone::Active,
            count: total,
            detail: None,
        })
    }

    /// Calm indicator for persisted `pending_router_followups` waiting behind an
    /// active Router reply (Q5).
    pub(crate) fn router_followup_badge(&self) -> Option<AnyElement> {
        let active_id = self.agent.active_session_id()?;
        if self.agent.session_role(&active_id) != SessionRole::Router {
            return None;
        }
        let followups = self.agent.pending_router_followups(&active_id);
        if followups.is_empty() {
            return None;
        }
        let label = self.tf(
            "chat.router_followup_queued",
            &[("count", &followups.len().to_string())],
        );
        let preview = followups.first().map(|item| {
            let safe = crate::ports::redact_trace_text(&item.text);
            crate::ports::clip_label(&safe, 24)
        });
        let display = match preview.as_deref() {
            Some(text) if !text.is_empty() => {
                format!("{label} \u{00b7} {text}")
            }
            _ => label.clone(),
        };
        Some(
            div()
                .id("router-followup-pill")
                .role(gpui::Role::Status)
                .aria_label(label)
                .px(self.theme.space(8.))
                .py(px(3.))
                .rounded_full()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.soft())
                .text_xs()
                .text_color(self.theme.muted())
                .max_w(px(260.))
                .overflow_hidden()
                .whitespace_nowrap()
                .truncate()
                .child(display)
                .into_any_element(),
        )
    }

    /// Two-section background works popover: `This conversation` + `Other
    /// conversations`, deduplicated by `work_id`, reading cached `popover_works`
    /// in O(1) with click-to-inspect in parent Router and per-work Stop (Q2 & Q6).
    pub(crate) fn router_bg_tasks_popover(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let snap = &self.popover_works;
        let show_anonymous_active = self.show_anonymous_active_work();
        let total = self.popover_total_count();

        let pad = self.theme.space(10.);
        let gap = self.theme.space(6.);
        let mut panel = theme::scroll_area("bg-tasks-scroll", &self.theme)
            .a11y_label(self.t("a11y.scroll_background_tasks"))
            .debug_selector(|| "bg-tasks-scroll".to_owned())
            .w_full()
            .max_h_viewport(px(380.))
            .scrollbar_inset(px(6.))
            .viewport_style(move |s| s.p(pad).flex().flex_col().gap(gap))
            .child(
                div()
                    .w_full()
                    .px(self.theme.space(4.))
                    .pb(self.theme.space(4.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.ink())
                            .child(self.t("chat.bg_tasks_title")),
                    )
                    .child(
                        div()
                            .px(self.theme.space(6.))
                            .py(px(1.))
                            .rounded_full()
                            .bg(self.theme.soft())
                            .text_xs()
                            .text_color(self.theme.muted())
                            .child(total.to_string()),
                    ),
            );

        if !snap.current_works.is_empty() {
            panel =
                panel.child(self.bg_section_heading("chat.bg_tasks_this_chat"));
            for (ix, work) in snap.current_works.iter().enumerate() {
                panel = panel.child(self.render_child_work_row(
                    ("bg-cur-work", ix as u64),
                    work,
                    None,
                    cx,
                ));
            }
        }

        let has_others = !snap.other_works.is_empty()
            || !snap.legacy_running_sessions.is_empty()
            || !snap.legacy_receipts.is_empty();
        if has_others {
            if !snap.current_works.is_empty() {
                panel = panel.child(
                    self.bg_section_heading("chat.bg_tasks_other_chats"),
                );
            }
            for (ix, (parent_title, work)) in
                snap.other_works.iter().enumerate()
            {
                panel = panel.child(self.render_child_work_row(
                    ("bg-other-work", ix as u64),
                    work,
                    Some(parent_title.as_str()),
                    cx,
                ));
            }
            for (ix, (session_id, title)) in
                snap.legacy_running_sessions.iter().enumerate()
            {
                panel = panel.child(self.render_legacy_running_row(
                    ix as u64, session_id, title, cx,
                ));
            }
            for (ix, (receipt, state)) in
                snap.legacy_receipts.iter().enumerate()
            {
                panel =
                    panel.child(self.render_legacy_receipt_row(
                        ix as u64, receipt, *state, cx,
                    ));
            }
        }

        if show_anonymous_active {
            let status_text = self.t("chat.status_running");
            panel = panel.child(
                div()
                    .id("bg-task-active-status")
                    .role(gpui::Role::Status)
                    .aria_label(status_text.clone())
                    .w_full()
                    .min_w_0()
                    .px(self.theme.space(8.))
                    .py(self.theme.space(6.))
                    .rounded_lg()
                    .bg(self.theme.soft().opacity(0.45))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(self.theme.status_active()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.ink())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(status_text),
                    )
                    .child(theme::status_pill(
                        self.theme.status_active_bg(),
                        self.theme.status_active(),
                        self.t("chat.bg_task_running"),
                        None,
                    )),
            );
        }

        if total == 0 {
            panel = panel.child(
                div()
                    .py(self.theme.space(12.))
                    .text_xs()
                    .text_color(self.theme.muted())
                    .text_center()
                    .child(self.t("chat.bg_tasks_empty")),
            );
        }

        div()
            .id("bg-tasks-panel")
            .occlude()
            .w(px(380.))
            .bg(self.theme.card())
            .border_1()
            .border_color(self.theme.line())
            .rounded_xl()
            .shadow_md()
            .overflow_hidden()
            .child(panel)
    }

    fn bg_section_heading(&self, key: &'static str) -> impl IntoElement {
        div()
            .px(self.theme.space(4.))
            .pt(px(2.))
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(self.theme.muted())
            .child(self.t(key))
    }

    fn render_legacy_running_row(
        &self,
        ix: u64,
        session_id: &str,
        title: &str,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let open_sid = session_id.to_owned();
        let inspect_title = title.to_owned();
        let stop_sid = session_id.to_owned();
        div()
            .id(("bg-legacy-row", ix))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(self.theme.space(4.))
            .child(
                theme::interactive(
                    ("bg-task-running", ix),
                    inspect_title.clone(),
                    &self.theme,
                )
                .flex_1()
                .min_w_0()
                .px(self.theme.space(8.))
                .py(self.theme.space(6.))
                .rounded_lg()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(
                    div()
                        .size(px(6.))
                        .flex_none()
                        .rounded_full()
                        .bg(self.theme.status_active()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.ink())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(title.to_owned()),
                )
                .child(theme::status_pill(
                    self.theme.status_active_bg(),
                    self.theme.status_active(),
                    self.t("chat.bg_task_running"),
                    None,
                ))
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.open_work_inspector(
                            crate::deliverables::InspectedWork::Session {
                                id: open_sid.clone(),
                                title: inspect_title.clone(),
                            },
                            cx,
                        );
                    },
                )),
            )
            .child(
                theme::Button::new(
                    ("bg-legacy-stop", ix),
                    self.t("chat.bg_work_stop"),
                )
                .variant(theme::ControlVariant::Ghost)
                .size(theme::ControlSize::Small)
                .render(
                    &self.theme,
                    cx.listener(move |panel, _, _, cx| {
                        panel.stop_session_turn(&stop_sid, cx);
                    }),
                ),
            )
            .into_any_element()
    }

    fn render_legacy_receipt_row(
        &self,
        ix: u64,
        receipt: &AcceptedMessage,
        state: ReceiptState,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let safe_text = crate::ports::redact_trace_text(&receipt.text);
        let preview: String = safe_text.chars().take(40).collect();
        let owner = self.receipt_owner(receipt);
        let receipt_tag = self.tf(
            "chat.pending_receipt",
            &[("id", &crate::ports::short_id(&receipt.id))],
        );
        let label =
            format!("{receipt_tag} \u{00b7} {owner} \u{00b7} {preview}");
        let meta = format!("{receipt_tag} \u{00b7} {owner}");
        let heading = if preview.trim().is_empty() {
            receipt_tag
        } else {
            preview
        };
        let (state_fg, state_label, _) = self.receipt_state_visual(state);
        let inspect_receipt = receipt.clone();
        theme::interactive(("pending-receipt", ix), label, &self.theme)
            .w_full()
            .min_w_0()
            .px(self.theme.space(8.))
            .py(self.theme.space(6.))
            .rounded_lg()
            .bg(self.theme.soft().opacity(0.45))
            .cursor_pointer()
            .hover(|style| style.bg(self.theme.soft()))
            .flex()
            .items_center()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.ink())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(heading),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.muted())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(meta),
                    ),
            )
            .child(theme::status_pill(
                self.theme.soft(),
                state_fg,
                state_label,
                None,
            ))
            .on_click(cx.listener(move |panel, _, _, cx| {
                panel.open_work_inspector(
                    crate::deliverables::InspectedWork::Receipt(
                        inspect_receipt.clone(),
                    ),
                    cx,
                );
            }))
            .into_any_element()
    }

    pub(crate) fn render_child_work_row(
        &self,
        id: (&'static str, u64),
        work: &ChildWorkSummary,
        parent_label: Option<&str>,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let title = if work.title.trim().is_empty() {
            crate::ports::short_id(&work.work_id)
        } else {
            crate::ports::redact_trace_text(&work.title)
        };
        let heartbeat_detail = work.heartbeat.as_ref().and_then(|hb| {
            crate::pending_card::format_heartbeat_line_localized(hb, &|key| {
                self.t(key)
            })
        });
        let subtitle = match (parent_label, heartbeat_detail) {
            (Some(parent), Some(hb)) => Some(format!("{parent} \u{00b7} {hb}")),
            (Some(parent), None) => Some(parent.to_owned()),
            (None, hb) => hb,
        };
        let (dot_color, pill_bg, pill_fg, state_text) =
            match work.state.as_str() {
                "awaiting_approval" => (
                    self.theme.status_attention(),
                    self.theme.status_attention_bg(),
                    self.theme.status_attention(),
                    self.t("board.state_needs_approval"),
                ),
                "failed" | "partial" => (
                    self.theme.danger(),
                    self.theme.danger_bg(),
                    self.theme.danger(),
                    self.t("dock.attention_badge"),
                ),
                "queued" => (
                    self.theme.muted(),
                    self.theme.soft(),
                    self.theme.muted(),
                    self.t("chat.bg_work_queued"),
                ),
                "stopping" => (
                    self.theme.muted(),
                    self.theme.soft(),
                    self.theme.muted(),
                    self.t("chat.status_stopping"),
                ),
                "delivered" | "completed" => (
                    self.theme.status_verified(),
                    self.theme.status_verified_bg(),
                    self.theme.status_verified(),
                    self.t("verify.card.passed"),
                ),
                _ => (
                    self.theme.status_active(),
                    self.theme.status_active_bg(),
                    self.theme.status_active(),
                    self.t("chat.bg_task_running"),
                ),
            };
        let cancellable = is_work_cancellable(&work.state);
        let root_open = work.root_session_id.clone();
        let wid_open = work.work_id.clone();
        let title_open = title.clone();
        let root_stop = work.root_session_id.clone();
        let wid_stop = work.work_id.clone();
        let stop_btn_id = SharedString::from(format!("{}-stop-{}", id.0, id.1));

        div()
            .id((id.0, id.1))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(self.theme.space(4.))
            .child(
                theme::interactive(
                    SharedString::from(format!("{}-open-{}", id.0, id.1)),
                    title.clone(),
                    &self.theme,
                )
                .flex_1()
                .min_w_0()
                .px(self.theme.space(8.))
                .py(self.theme.space(6.))
                .rounded_lg()
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(
                    div().size(px(6.)).flex_none().rounded_full().bg(dot_color),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(title),
                        )
                        .when_some(subtitle, |col, text| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(text),
                            )
                        }),
                )
                .child(theme::status_pill(pill_bg, pill_fg, state_text, None))
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.open_child_work_in_parent(
                            &root_open,
                            &wid_open,
                            title_open.clone(),
                            cx,
                        );
                    },
                )),
            )
            .when(cancellable, |row| {
                row.child(
                    theme::Button::new(
                        stop_btn_id,
                        self.t("chat.bg_work_stop"),
                    )
                    .variant(theme::ControlVariant::Ghost)
                    .size(theme::ControlSize::Small)
                    .render(
                        &self.theme,
                        cx.listener(move |panel, _, _, cx| {
                            panel.cancel_child_work(&root_stop, &wid_stop, cx);
                        }),
                    ),
                )
            })
            .into_any_element()
    }
}
